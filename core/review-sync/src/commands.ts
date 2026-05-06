/** CLI-compatible review-sync command runner. */
import { parse, runSafely, subcommands } from "cmd-ts"

import { cleanupReviewSessions, createCleanupCommand } from "./commands/cleanup.ts"
import { createPauseCommand, pauseReviewSession } from "./commands/pause.ts"
import { createResumeCommand, resumeReviewSession } from "./commands/resume.ts"
import { createStartCommand, startReviewSync } from "./commands/start.ts"
import { createStatusCommand, statusReviewSession } from "./commands/status.ts"
import { createSyncCommand, syncReviewSession } from "./commands/sync.ts"
import { createWatchCommand, watchReviewSession } from "./commands/watch.ts"
import { createErrorResult, createReviewSyncResult } from "./errors.ts"
import type { ReviewSyncCommand } from "./types.ts"

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
  const command = createReviewSyncCommand(process.cwd())
  let selectedCommand: ReviewSyncCommand | null = null

  try {
    selectedCommand = await parseReviewSyncCommandName(command, argv)
    const parsed = await runSafely(command, argv)
    return parsed._tag === "error" ? createCmdTsResult(parsed.error) : await parsed.value.value
  } catch (error) {
    return createErrorResult(selectedCommand ?? "status", error)
  }
}

/** Converts cmd-ts parse/help exits into the public structured result shape. */
function createCmdTsResult(error: { config: { exitCode: number; message: string } }) {
  return createReviewSyncResult({
    exitCode: error.config.exitCode,
    command: "status",
    status: error.config.exitCode === 0 ? "ok" : "error",
    message: error.config.message,
  })
}

/** Identifies the subcommand before running so handler errors keep the right command label. */
async function parseReviewSyncCommandName(
  command: ReturnType<typeof createReviewSyncCommand>,
  argv: string[],
) {
  const parsed = await parse(command, argv)
  if (parsed._tag === "ok") {
    return parsed.value.command as ReviewSyncCommand
  }

  const partialValue = parsed.error.partialValue
  if (isParsedReviewSyncCommand(partialValue)) {
    return partialValue.command
  }
  return null
}

/** Narrows cmd-ts partial parse values without depending on its internal error shape. */
function isParsedReviewSyncCommand(value: unknown): value is { command: ReviewSyncCommand } {
  if (!value || typeof value !== "object" || !("command" in value)) {
    return false
  }

  return isReviewSyncCommand((value as { command: unknown }).command)
}

/** Checks command names at the CLI boundary where argv is still untrusted. */
function isReviewSyncCommand(command: unknown): command is ReviewSyncCommand {
  return (
    command === "start" ||
    command === "sync" ||
    command === "status" ||
    command === "pause" ||
    command === "resume" ||
    command === "cleanup" ||
    command === "watch"
  )
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
