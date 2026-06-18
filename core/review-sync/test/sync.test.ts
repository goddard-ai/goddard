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
  readSessionStates,
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

test("sync advances the review branch after rendered agent changes are committed", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.agentDir, "shared.txt"), "agent edit\n")
  const rendered = await syncReviewSession({
    cwd: fixture.agentDir,
  })
  expect(rendered.status).toBe("ok")
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe(
    "M  shared.txt\n",
  )

  await runGit(fixture.agentDir, ["add", "shared.txt"])
  await runGit(fixture.agentDir, ["commit", "-m", "agent edit"])
  const committed = await syncReviewSession({
    cwd: fixture.agentDir,
  })

  const agentHead = (
    await runGit(fixture.agentDir, ["rev-parse", "refs/heads/codex/review-sync-test"])
  ).stdout.trim()
  expect(committed.status).toBe("ok")
  expect(committed.acceptedPatchPath).toBeUndefined()
  expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(agentHead)
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
})

test("sync preserves uncommitted agent changes after a partial agent commit", async () => {
  const fixture = await createStartedFixture({
    "committed.txt": "base\n",
    "uncommitted.txt": "base\n",
  })

  await writeText(join(fixture.agentDir, "committed.txt"), "agent committed edit\n")
  await writeText(join(fixture.agentDir, "uncommitted.txt"), "agent uncommitted edit\n")
  const rendered = await syncReviewSession({
    cwd: fixture.agentDir,
  })
  expect(rendered.status).toBe("ok")

  await runGit(fixture.agentDir, ["add", "committed.txt"])
  await runGit(fixture.agentDir, ["commit", "-m", "agent partial edit"])
  const partiallyCommitted = await syncReviewSession({
    cwd: fixture.agentDir,
  })

  const agentHead = (
    await runGit(fixture.agentDir, ["rev-parse", "refs/heads/codex/review-sync-test"])
  ).stdout.trim()
  expect(partiallyCommitted.status).toBe("ok")
  expect(partiallyCommitted.acceptedPatchPath).toBeUndefined()
  expect(await readFile(join(fixture.agentDir, "uncommitted.txt"), "utf-8")).toBe(
    "agent uncommitted edit\n",
  )
  expect(await readFile(join(fixture.reviewDir, "uncommitted.txt"), "utf-8")).toBe(
    "agent uncommitted edit\n",
  )
  expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(agentHead)
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe(
    "M  uncommitted.txt\n",
  )
})

test("sync writes explanatory snapshot commit messages", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const [session] = await readSessionStates(fixture.agentDir)
  const snapshotRef = `refs/review-sync/${session!.sessionId}/agent-snapshot`

  const subject = (
    await runGit(fixture.agentDir, ["show", "-s", "--format=%s", snapshotRef])
  ).stdout.trim()
  const body = (await runGit(fixture.agentDir, ["show", "-s", "--format=%b", snapshotRef])).stdout

  expect(subject).toBe(`review-sync snapshot: ${session!.sessionId}:agent`)
  expect(body).toContain("compare the review worktree with the agent worktree")
  expect(body).toContain("refresh the disposable review branch")
  expect(body).toContain("review-sync may rewrite it")
})

test("sync applies clean human edits back to the agent worktree", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const reviewHeadBeforeSync = (
    await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])
  ).stdout.trim()

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(result.acceptedPatchPath).toBeTruthy()
  expect(existsSync(result.acceptedPatchPath!)).toBe(true)
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  expect(await readFile(join(fixture.reviewDir, "shared.txt"), "utf-8")).toBe("human edit\n")
  expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
    reviewHeadBeforeSync,
  )
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe(
    "M  shared.txt\n",
  )
})

test("sync applies committed review changes when the agent branch is checked out", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human commit\n")
  await runGit(fixture.reviewDir, ["add", "shared.txt"])
  await runGit(fixture.reviewDir, ["commit", "-m", "human review commit"])
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(result.acceptedPatchPath).toBeTruthy()
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("human commit\n")
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
})

test("sync advances agent HEAD when already-rendered review content is committed", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit before commit\n")
  const firstResult = await syncReviewSession({
    cwd: fixture.reviewDir,
  })
  expect(firstResult.status).toBe("ok")
  expect(firstResult.acceptedPatchPath).toBeTruthy()
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe(
    "human edit before commit\n",
  )
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe(
    "M  shared.txt\n",
  )

  const beforeCommitHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()
  await runGit(fixture.reviewDir, ["commit", "-m", "human review commit after sync"])
  const afterCommitHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()
  const secondResult = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(afterCommitHead).not.toBe(beforeCommitHead)
  expect(secondResult.status).toBe("ok")
  expect(secondResult.acceptedPatchPath).toBeUndefined()
  expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
    afterCommitHead,
  )
  expect((await runGit(fixture.reviewDir, ["log", "-1", "--format=%s"])).stdout.trim()).toBe(
    "human review commit after sync",
  )
  expect((await runGit(fixture.agentDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
    afterCommitHead,
  )
  expect((await runGit(fixture.agentDir, ["log", "-1", "--format=%s"])).stdout.trim()).toBe(
    "human review commit after sync",
  )
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe(
    "human edit before commit\n",
  )
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
  expect((await runGit(fixture.agentDir, ["status", "--porcelain=v1"])).stdout).toBe("")
})

test("sync preserves a cherry-picked review commit after accepting its patch", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })
  const sourceDir = join(fixture.rootDir, "source")
  await runGit(fixture.agentDir, ["worktree", "add", "-b", "human/source", sourceDir, "main"])
  await writeText(join(sourceDir, "shared.txt"), "picked edit\n")
  await runGit(sourceDir, ["add", "shared.txt"])
  await runGit(sourceDir, ["commit", "-m", "picked review edit"])
  const pickedCommit = (await runGit(sourceDir, ["rev-parse", "HEAD"])).stdout.trim()

  await runGit(fixture.reviewDir, ["cherry-pick", pickedCommit])
  const cherryPickedHead = (await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()
  const result = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(result.status).toBe("ok")
  expect(result.acceptedPatchPath).toBeTruthy()
  expect(await readFile(join(fixture.agentDir, "shared.txt"), "utf-8")).toBe("picked edit\n")
  expect((await runGit(fixture.reviewDir, ["rev-parse", "HEAD"])).stdout.trim()).toBe(
    cherryPickedHead,
  )
  expect((await runGit(fixture.reviewDir, ["log", "-1", "--format=%s"])).stdout.trim()).toBe(
    "picked review edit",
  )
  expect((await runGit(fixture.reviewDir, ["status", "--porcelain=v1"])).stdout).toBe("")
})

test("sync does not reapply the rendered baseline as another human patch", async () => {
  const fixture = await createStartedFixture({
    "shared.txt": "base\n",
  })

  await writeText(join(fixture.reviewDir, "shared.txt"), "human edit\n")
  const first = await syncReviewSession({
    cwd: fixture.reviewDir,
  })
  const second = await syncReviewSession({
    cwd: fixture.reviewDir,
  })

  expect(first.status).toBe("ok")
  expect(first.acceptedPatchPath).toBeTruthy()
  expect(second.status).toBe("ok")
  expect(second.acceptedPatchPath).toBeUndefined()
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
