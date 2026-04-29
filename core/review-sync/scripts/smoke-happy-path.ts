import assert from "node:assert/strict"
import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"

import { startReviewSync, statusReviewSession, syncReviewSession } from "../src/index.ts"

type SmokeFixture = {
  rootDir: string
  agentDir: string
  reviewDir: string
}

let rootDir: string | null = null

try {
  const fixture = await createFixture()
  rootDir = fixture.rootDir

  // Start once and keep the same review-sync session for every scenario below.
  // This is intentionally not a set of isolated fixtures: the smoke test should
  // catch regressions where accepted review patches, refreshed rendered
  // snapshots, or repeated syncs corrupt later sync behavior.
  const start = await startReviewSync({
    cwd: fixture.agentDir,
    reviewWorktree: fixture.reviewDir,
  })
  assert.equal(start.status, "ok", "start status")
  assert.equal(start.reviewBranch, "codex/review-sync-smoke--review", "review branch")
  assert.equal(
    await currentBranch(fixture.reviewDir),
    "codex/review-sync-smoke--review",
    "review worktree branch",
  )

  // Review-side tracked edits are the core feedback path. The review worktree
  // changes a file that existed in the rendered snapshot, sync captures the diff
  // from that rendered baseline, applies it into the agent worktree, then
  // refreshes the review worktree from the new agent snapshot.
  await runSmokeStep("review tracked file edit", async () => {
    await writeText(join(fixture.reviewDir, "shared.txt"), "review tracked edit\n")
    const sync = await syncReviewSession({
      cwd: fixture.reviewDir,
    })

    assert.equal(sync.status, "ok", "sync status")
    assert.ok(sync.acceptedPatchPath, "accepted patch path")
    assert.ok(existsSync(sync.acceptedPatchPath), "accepted patch file exists")
    await assertFileContent(fixture.agentDir, "shared.txt", "review tracked edit\n")
    await assertFileContent(fixture.reviewDir, "shared.txt", "review tracked edit\n")
    await assertPatchCounts(fixture, { accepted: 1, rejected: 0 })
  })

  // Review-side untracked files must be treated as human edits, not ignored as
  // mirror-only state. This protects the snapshotter contract that non-ignored
  // untracked files are included in review snapshots and accepted patches.
  await runSmokeStep("review untracked file add", async () => {
    await writeText(join(fixture.reviewDir, "review-only.txt"), "review untracked add\n")
    const sync = await syncReviewSession({
      cwd: fixture.reviewDir,
    })

    assert.equal(sync.status, "ok", "sync status")
    assert.ok(sync.acceptedPatchPath, "accepted patch path")
    assert.ok(existsSync(sync.acceptedPatchPath), "accepted patch file exists")
    await assertFileContent(fixture.agentDir, "review-only.txt", "review untracked add\n")
    await assertFileContent(fixture.reviewDir, "review-only.txt", "review untracked add\n")
    await assertPatchCounts(fixture, { accepted: 2, rejected: 0 })
  })

  // A human may commit inside the disposable review branch before syncing. The
  // package does not preserve that independent review history; it converts the
  // resulting worktree state into a patch for the agent, then resets the review
  // branch back to the agent snapshot.
  await runSmokeStep("review committed change", async () => {
    await writeText(join(fixture.reviewDir, "shared.txt"), "review committed edit\n")
    await runGit(fixture.reviewDir, ["add", "shared.txt"])
    await runGit(fixture.reviewDir, ["commit", "-m", "review edit"])
    const sync = await syncReviewSession({
      cwd: fixture.reviewDir,
    })

    assert.equal(sync.status, "ok", "sync status")
    assert.ok(sync.acceptedPatchPath, "accepted patch path")
    assert.ok(existsSync(sync.acceptedPatchPath), "accepted patch file exists")
    await assertFileContent(fixture.agentDir, "shared.txt", "review committed edit\n")
    await assertFileContent(fixture.reviewDir, "shared.txt", "review committed edit\n")
    await assertPatchCounts(fixture, { accepted: 3, rejected: 0 })
  })

  // Agent-side tracked edits exercise the mirror refresh path without a human
  // patch. The accepted patch count must not change because the agent worktree
  // is canonical and its local edits are rendered directly into the review
  // branch snapshot.
  await runSmokeStep("agent tracked file edit", async () => {
    await writeText(join(fixture.agentDir, "shared.txt"), "agent tracked edit\n")
    const sync = await syncReviewSession({
      cwd: fixture.agentDir,
    })

    assert.equal(sync.status, "ok", "sync status")
    assert.equal(sync.acceptedPatchPath, undefined, "accepted patch path")
    await assertFileContent(fixture.agentDir, "shared.txt", "agent tracked edit\n")
    await assertFileContent(fixture.reviewDir, "shared.txt", "agent tracked edit\n")
    await assertPatchCounts(fixture, { accepted: 3, rejected: 0 })
  })

  // Agent-side untracked files must also appear in the review worktree. This
  // proves the agent snapshot path includes non-ignored untracked files, instead
  // of only committed or tracked content from the agent branch.
  await runSmokeStep("agent untracked file add", async () => {
    await writeText(join(fixture.agentDir, "agent-only.txt"), "agent untracked add\n")
    const sync = await syncReviewSession({
      cwd: fixture.agentDir,
    })

    assert.equal(sync.status, "ok", "sync status")
    assert.equal(sync.acceptedPatchPath, undefined, "accepted patch path")
    await assertFileContent(fixture.agentDir, "agent-only.txt", "agent untracked add\n")
    await assertFileContent(fixture.reviewDir, "agent-only.txt", "agent untracked add\n")
    await assertPatchCounts(fixture, { accepted: 3, rejected: 0 })
  })

  // Agent commits should mirror just like uncommitted agent edits. This guards
  // the branch/reset path: the review branch should end at the latest agent
  // snapshot regardless of whether the agent change is committed or only present
  // in the working tree.
  await runSmokeStep("agent committed change", async () => {
    await writeText(join(fixture.agentDir, "shared.txt"), "agent committed edit\n")
    await runGit(fixture.agentDir, ["add", "shared.txt"])
    await runGit(fixture.agentDir, ["commit", "-m", "agent edit"])
    const sync = await syncReviewSession({
      cwd: fixture.agentDir,
    })

    assert.equal(sync.status, "ok", "sync status")
    assert.equal(sync.acceptedPatchPath, undefined, "accepted patch path")
    await assertFileContent(fixture.agentDir, "shared.txt", "agent committed edit\n")
    await assertFileContent(fixture.reviewDir, "shared.txt", "agent committed edit\n")
    await assertPatchCounts(fixture, { accepted: 3, rejected: 0 })
  })

  // Only clean up after every assertion passes. On failure, the catch block
  // prints the temp root so a maintainer can inspect the Git refs, patch files,
  // worktree contents, and durable state that caused the smoke failure.
  await rm(fixture.rootDir, { recursive: true, force: true })
  rootDir = null
  console.log("review-sync smoke happy path passed")
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  if (rootDir) {
    console.error(`smoke fixture preserved at ${rootDir}`)
  }
  process.exitCode = 1
}

async function runSmokeStep(name: string, run: () => Promise<void>) {
  await run()
  console.log(`passed: ${name}`)
}

async function createFixture() {
  const rootDir = await mkdtemp(join(tmpdir(), "review-sync-smoke-"))
  const agentDir = join(rootDir, "agent")
  const reviewDir = join(rootDir, "review")

  // Create a real repository and a real secondary worktree. The smoke test is
  // meant to validate Git behavior at the integration boundary, including branch
  // checkout rules and worktree branch exclusivity, so synthetic filesystem
  // fixtures would miss the failures this script is meant to catch.
  await mkdir(agentDir, { recursive: true })
  await runGit(agentDir, ["init", "-b", "main"])
  await runGit(agentDir, ["config", "user.email", "smoke@example.com"])
  await runGit(agentDir, ["config", "user.name", "Smoke Test"])
  await writeText(join(agentDir, "shared.txt"), "base\n")
  await runGit(agentDir, ["add", "."])
  await runGit(agentDir, ["commit", "-m", "init"])
  await runGit(agentDir, ["checkout", "-B", "codex/review-sync-smoke"])
  await runGit(agentDir, ["worktree", "add", "--detach", reviewDir, "HEAD"])

  return {
    rootDir,
    agentDir,
    reviewDir,
  } satisfies SmokeFixture
}

async function assertFileContent(cwd: string, path: string, expected: string) {
  assert.equal(await readFile(join(cwd, path), "utf-8"), expected, `${path} content in ${cwd}`)
}

async function assertPatchCounts(
  fixture: SmokeFixture,
  expected: {
    accepted: number
    rejected: number
  },
) {
  // Patch counts are part of the behavioral contract here. Review-originated
  // changes should create accepted patch files, while agent-originated changes
  // should only refresh the rendered snapshot and leave the accepted/rejected
  // patch inventory unchanged.
  const status = await statusReviewSession({
    cwd: fixture.agentDir,
    json: true,
  })
  assert.equal(status.status, "ok", "status command")
  const statusPayload = JSON.parse(status.message) as {
    patchCounts?: {
      accepted?: number
      rejected?: number
    }
  }
  assert.equal(statusPayload.patchCounts?.accepted, expected.accepted, "accepted patch count")
  assert.equal(statusPayload.patchCounts?.rejected, expected.rejected, "rejected patch count")
}

async function writeText(path: string, content: string) {
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, content, "utf-8")
}

async function currentBranch(cwd: string) {
  return (await runGit(cwd, ["symbolic-ref", "--short", "HEAD"])).stdout.trim()
}

async function runGit(cwd: string, args: string[]) {
  return await new Promise<{
    status: number
    stdout: string
    stderr: string
  }>((resolvePromise, rejectPromise) => {
    const child = spawn("git", args, {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    })

    let stdout = ""
    let stderr = ""
    child.stdout.setEncoding("utf8")
    child.stderr.setEncoding("utf8")
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })
    child.on("error", rejectPromise)
    child.on("close", (status) => {
      const result = {
        status: status ?? 1,
        stdout,
        stderr,
      }
      if (result.status !== 0) {
        rejectPromise(
          new Error(
            `git ${args.join(" ")} failed in ${cwd}: ${
              result.stderr.trim() || result.stdout.trim()
            }`,
          ),
        )
        return
      }
      resolvePromise(result)
    })
  })
}
