import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { createWorktree, deleteWorktree } from "../src/worktrees/index.ts"
import {
  findMountedReviewSyncSessionByPrimaryDir,
  ReviewSyncWorktreeSessionHost,
} from "../src/worktrees/review-sync.ts"

const cleanup: string[] = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("review-sync host mounts, rehydrates, and unmounts a daemon worktree", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-review-sync-a",
  })
  cleanup.push(created.worktreeDir)

  await writeFile(join(repoDir, "shared.txt"), "primary pre-mount\n", "utf-8")
  await writeFile(join(repoDir, "primary-note.txt"), "keep me\n", "utf-8")
  await writeFile(join(created.worktreeDir, "shared.txt"), "worktree dirty\n", "utf-8")
  await writeFile(join(created.worktreeDir, "worktree-note.txt"), "mirror me\n", "utf-8")

  const host = new ReviewSyncWorktreeSessionHost({
    sessionId: "ses_review_sync_mount",
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
  })

  const mounted = await host.mount()
  const expectedPrimaryDir = await realpath(repoDir)
  const expectedWorktreeDir = await realpath(created.worktreeDir)
  expect(mounted?.status).toBe("mounted")
  expect(mounted?.primaryDir).toBe(expectedPrimaryDir)
  expect(mounted?.worktreeDir).toBe(expectedWorktreeDir)
  expect(mounted?.worktreeLatestSnapshotOid).toMatch(/^[0-9a-f]{40}$/)
  expect(await currentBranch(repoDir)).toBe("review-sync/goddard-review-sync-a")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("worktree dirty\n")
  expect(await readFile(join(repoDir, "worktree-note.txt"), "utf-8")).toBe("mirror me\n")

  const found = await findMountedReviewSyncSessionByPrimaryDir(repoDir)
  expect(found?.sessionId).toBe("ses_review_sync_mount")

  const rehydrated = await new ReviewSyncWorktreeSessionHost({
    sessionId: "ses_review_sync_mount",
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
  }).inspect()
  expect(rehydrated?.status).toBe("mounted")
  expect(rehydrated?.baseOid).toBe(mounted?.baseOid)

  const reused = await host.mount()
  expect(reused?.baseOid).toBe(mounted?.baseOid)

  const unmounted = await host.unmount()
  expect(unmounted.warnings).toEqual([])
  expect(await currentBranch(repoDir)).toBe("main")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("primary pre-mount\n")
  expect(await readFile(join(repoDir, "primary-note.txt"), "utf-8")).toBe("keep me\n")
  expect(await host.inspect()).toBeNull()

  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
})

async function createRepoFixture(files: Record<string, string>) {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-review-sync-host-repo-"))
  cleanup.push(repoDir)

  for (const [relativePath, content] of Object.entries(files)) {
    await writeFile(join(repoDir, relativePath), content, "utf-8")
  }

  await runGit(repoDir, ["init", "-b", "main"])
  await runGit(repoDir, ["config", "user.email", "bot@example.com"])
  await runGit(repoDir, ["config", "user.name", "Bot"])
  await runGit(repoDir, ["add", "."])
  await runGit(repoDir, ["commit", "-m", "init"])

  expect(existsSync(repoDir)).toBe(true)
  return repoDir
}

async function currentBranch(cwd: string) {
  return (await runGit(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"])).stdout.trim()
}

async function runGit(cwd: string, args: string[]) {
  const result = await new Promise<{
    status: number | null
    stdout: string
    stderr: string
  }>((resolve, reject) => {
    const child = spawn("git", args, {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    })

    if (!child.stdout || !child.stderr) {
      reject(new Error("Failed to capture git output"))
      return
    }

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

    child.on("error", reject)
    child.on("close", (status) => {
      resolve({ status, stdout, stderr })
    })
  })

  expect({
    status: result.status,
    stdout: result.stdout,
    stderr: result.stderr,
  }).toMatchObject({
    status: 0,
  })
  return result
}
