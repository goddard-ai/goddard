import { existsSync } from "node:fs"
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { pauseReviewSession, resumeReviewSession, syncReviewSession } from "../src/index.ts"
import {
  captureReviewSyncError,
  cleanupReviewSyncFixtures,
  createStartedFixture,
  gitDir,
  runGit,
  writeText,
} from "./support.ts"

afterEach(cleanupReviewSyncFixtures)

test("sync mirrors agent uncommitted changes through the review branch", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  const result = await syncReviewSession({
    cwd: fixture.agentDir,
  })

  expect(result.status).toBe("ok")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
})

test("sync applies clean human edits back to the agent worktree", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(result.acceptedPatchPath).toBeTruthy()
  expect(existsSync(result.acceptedPatchPath!)).toBe(true)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("sync fails when the agent worktree is not on the expected branch", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  await runGit(fixture.agentDir, ["checkout", "-B", "codex/temporary"])

  const error = await captureReviewSyncError(() =>
    syncReviewSession({
      cwd: fixture.reviewDir,
    }),
  )

  expect(error.status).toBe("error")
  expect(error.message).toContain("must be on codex/review-sync-test; currently codex/temporary")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("base\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})

test("sync ignores stale REBASE_HEAD when Git reports no active rebase", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const reviewGitDir = await gitDir(fixture.reviewDir)
  const reviewHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()
  await writeText(join(reviewGitDir, "REBASE_HEAD"), `${reviewHead}\n`)

  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(existsSync(join(reviewGitDir, "REBASE_HEAD"))).toBe(true)
})

test("sync preserves rejected human patches and refreshes review from the agent", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("rejected-human-patch")
  expect(result.rejectedPatchPath).toBeTruthy()
  expect(await readFile(result.rejectedPatchPath!, "utf-8")).toContain("human edit")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("agent edit\n")
})

test("sync includes untracked non-ignored files and excludes ignored files", async () => {
  const fixture = await createStartedFixture({
    ".gitignore": "ignored.txt\n",
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "new-file.txt"), "include me\n")
  await writeText(join(fixture.reviewDir, "ignored.txt"), "do not include\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(await readFile(join(fixture.agentDir, "new-file.txt"), "utf-8")).toBe("include me\n")
  expect(existsSync(join(fixture.agentDir, "ignored.txt"))).toBe(false)
})

test("pause blocks sync mutations until resume", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  const pause = await pauseReviewSession({
    cwd: fixture.agentDir,
  })
  expect(pause.status).toBe("paused")

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const pausedSyncError = await captureReviewSyncError(() =>
    syncReviewSession({
      cwd: fixture.reviewDir,
    }),
  )
  expect(pausedSyncError.status).toBe("paused")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("base\n")

  const resume = await resumeReviewSession({
    cwd: fixture.agentDir,
  })
  expect(resume.status).toBe("ok")

  const synced = await syncReviewSession({
    cwd: fixture.reviewDir,
  })
  expect(synced.status).toBe("ok")
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
})
