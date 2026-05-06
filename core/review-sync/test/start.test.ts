import { afterEach, expect, test } from "bun:test"

import { startReviewSync } from "../src/index.ts"
import {
  cleanupReviewSyncFixtures,
  cliPath,
  createFixture,
  currentBranch,
  runGit,
  runProcess,
} from "./support.ts"

afterEach(cleanupReviewSyncFixtures)

test("start derives and checks out the review branch", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })

  expect(result.status).toBe("ok")
  expect(result.reviewBranch).toBe("review-sync/codex/review-sync-test")
  expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
})

test("start refuses agent branches that already look like review branches", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "review-sync/codex/already"])

  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "review-sync/codex/already",
  })

  expect(result.status).toBe("error")
  expect(result.exitCode).toBe(1)
  expect(result.message).toContain("already starts with review-sync/")
})

test("start refuses branches not checked out in another worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/other-agent"])

  const result = await startReviewSync({
    cwd: fixture.reviewDir,
    agentBranch: "codex/review-sync-test",
  })

  expect(result.status).toBe("error")
  expect(result.exitCode).toBe(1)
  expect(result.message).toContain("is not checked out in another worktree")
})

test("cli start accepts an agent branch from the review worktree", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await runProcess(fixture.reviewDir, "bun", [
    cliPath,
    "start",
    "codex/review-sync-test",
  ])

  expect(result.status).toBe(0)
  expect(result.stdout).toContain("Started review sync")
  expect(await currentBranch(fixture.reviewDir)).toBe("review-sync/codex/review-sync-test")
})

test("cli start requires an agent branch when non-interactive", async () => {
  const fixture = await createFixture({
    "shared.txt": "base\n",
  })

  const result = await runProcess(fixture.reviewDir, "bun", [cliPath, "start"])

  expect(result.status).toBe(1)
  expect(result.stderr).toContain("start requires an agent branch")
})
