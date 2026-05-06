/** CLI-compatible review-sync command runner. */
import { runSafely, subcommands } from "cmd-ts"

import { cleanupReviewSessions, createCleanupCommand } from "./commands/cleanup.ts"
import { createPauseCommand, pauseReviewSession } from "./commands/pause.ts"
import { createResumeCommand, resumeReviewSession } from "./commands/resume.ts"
import { createCmdTsResult } from "./commands/shared.ts"
import { createStartCommand, startReviewSync } from "./commands/start.ts"
import { createStatusCommand, statusReviewSession } from "./commands/status.ts"
import { createSyncCommand, syncReviewSession } from "./commands/sync.ts"
import { createWatchCommand, watchReviewSession } from "./commands/watch.ts"
import { createErrorResult } from "./errors.ts"

export {
  cleanupReviewSessions,
  pauseReviewSession,
  resumeReviewSession,
  startReviewSync,
  statusReviewSession,
  syncReviewSession,
  watchReviewSession,
}

/** Runs one review-sync command using the same command names and process context as the CLI. */
export async function runReviewSync(argv: string[]) {
  try {
    const parsed = await runSafely(createReviewSyncCommand(process.cwd()), argv)
    return parsed._tag === "error" ? createCmdTsResult(parsed.error) : await parsed.value.value
  } catch (error) {
    return createErrorResult("status", error)
  }
}

/** Builds the command tree with handlers that execute the bounded operations directly. */
function createReviewSyncCommand(cwd: string) {
  return subcommands({
    name: "review-sync",
    description: "Synchronize an agent-owned branch with a disposable human review branch",
    cmds: {
      start: createStartCommand(cwd),
      sync: createSyncCommand(cwd),
      status: createStatusCommand(cwd),
      pause: createPauseCommand(cwd),
      resume: createResumeCommand(cwd),
      cleanup: createCleanupCommand(cwd),
      watch: createWatchCommand(cwd),
    },
  })
}
