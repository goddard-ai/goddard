import { mkdir, readdir, readFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { cleanupReviewSessions, startReviewSync, statusReviewSession } from "../src/index.ts"
import {
  captureReviewSyncError,
  cleanupReviewSyncFixtures,
  cliPath,
  createFixture,
  createStartedFixture,
  readSessionStates,
  refExists,
  runGit,
  runProcess,
  sleep,
  writeText,
} from "./support.ts"

afterEach(cleanupReviewSyncFixtures)

test("status explains recovery when multiple sessions match the worktree", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const commonDir = (
    await runGit(fixture.agentDir, ["rev-parse", "--path-format=absolute", "--git-common-dir"])
  ).stdout.trim()
  const sessionsRoot = join(commonDir, "review-sync", "sessions")
  const [sessionId] = await readdir(sessionsRoot)
  const state = JSON.parse(
    await readFile(join(sessionsRoot, sessionId!, "state.json"), "utf-8"),
  ) as Record<string, unknown>
  const duplicateSessionId = "sha256-duplicate-session"
  await mkdir(join(sessionsRoot, duplicateSessionId), { recursive: true })
  await writeText(
    join(sessionsRoot, duplicateSessionId, "state.json"),
    `${JSON.stringify(
      {
        ...state,
        sessionId: duplicateSessionId,
        agentBranch: "codex/second-review-sync-test",
        reviewBranch: "review-sync/codex/second-review-sync-test",
        refs: {
          agentSnapshot: `refs/review-sync/${duplicateSessionId}/agent-snapshot`,
          renderedSnapshot: `refs/review-sync/${duplicateSessionId}/rendered-snapshot`,
        },
      },
      null,
      2,
    )}\n`,
  )

  const error = await captureReviewSyncError(() =>
    statusReviewSession({
      cwd: fixture.agentDir,
    }),
  )

  expect(error.status).toBe("error")
  expect(error.message).toContain("Multiple review-sync sessions match")
  expect(error.message).toContain("codex/review-sync-test -> review-sync/codex/review-sync-test")
  expect(error.message).toContain(
    "codex/second-review-sync-test -> review-sync/codex/second-review-sync-test",
  )
  expect(error.message).toContain(`move stale session dirs out of ${sessionsRoot}`)
  expect(error.message).toContain("accepted/rejected patches live under each state dir")
})

test("cleanup removes older sessions for the resolved worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  const secondAgentDir = join(fixture.rootDir, "agent-two")
  await runGit(fixture.agentDir, ["branch", "codex/second-review-sync-test", "main"])
  await runGit(fixture.agentDir, [
    "worktree",
    "add",
    secondAgentDir,
    "codex/second-review-sync-test",
  ])

  const first = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })
  expect(first.status).toBe("ok")
  if (!first.sessionId) {
    throw new Error("first session did not report an id")
  }
  await sleep(5)

  const second = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/second-review-sync-test",
  })
  expect(second.status).toBe("ok")
  if (!second.sessionId) {
    throw new Error("second session did not report an id")
  }
  expect((await readSessionStates(fixture.reviewDir)).length).toBe(2)
  expect(
    await refExists(fixture.reviewDir, `refs/review-sync/${first.sessionId}/agent-snapshot`),
  ).toBe(true)

  const cleanup = await cleanupReviewSessions({
    cwd: fixture.reviewDir,
  })

  expect(cleanup.status).toBe("ok")
  expect(cleanup.sessionId).toBe(second.sessionId)
  expect(cleanup.message).toContain("Removed 1 review-sync session")
  expect(cleanup.message).toContain("Kept")
  const sessions = await readSessionStates(fixture.reviewDir)
  expect(sessions.map((session) => session.sessionId)).toEqual([second.sessionId])
  expect(
    await refExists(fixture.reviewDir, `refs/review-sync/${first.sessionId}/agent-snapshot`),
  ).toBe(false)
  expect(
    await refExists(fixture.reviewDir, `refs/review-sync/${second.sessionId}/agent-snapshot`),
  ).toBe(true)

  const status = await statusReviewSession({
    cwd: fixture.reviewDir,
  })
  expect(status.status).toBe("ok")
  expect(status.sessionId).toBe(second.sessionId)
})

test("cli cleanup -A removes every session for the resolved worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  const secondAgentDir = join(fixture.rootDir, "agent-two")
  await runGit(fixture.agentDir, ["branch", "codex/second-review-sync-test", "main"])
  await runGit(fixture.agentDir, [
    "worktree",
    "add",
    secondAgentDir,
    "codex/second-review-sync-test",
  ])

  const first = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })
  expect(first.status).toBe("ok")
  if (!first.sessionId) {
    throw new Error("first session did not report an id")
  }
  const second = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/second-review-sync-test",
  })
  expect(second.status).toBe("ok")
  if (!second.sessionId) {
    throw new Error("second session did not report an id")
  }

  const result = await runProcess(fixture.reviewDir, "bun", [cliPath, "cleanup", "-A"])

  expect(result.status).toBe(0)
  expect(result.stdout).toContain("Removed 2 review-sync sessions")
  expect(await readSessionStates(fixture.reviewDir)).toEqual([])
  expect(
    await refExists(fixture.reviewDir, `refs/review-sync/${first.sessionId}/agent-snapshot`),
  ).toBe(false)
  expect(
    await refExists(fixture.reviewDir, `refs/review-sync/${second.sessionId}/agent-snapshot`),
  ).toBe(false)
})
