import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { createWorktree, deleteWorktree } from "../src/worktrees/index.ts"
import {
  findMountedReviewSessionByPrimaryDir,
  mountReviewSession,
  syncReviewSessionOnce,
  unmountReviewSession,
} from "../src/worktrees/review-sync.ts"

const cleanup: string[] = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("review session adapter mounts, rehydrates, and unmounts through review-sync", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-review-sync-a",
  })
  cleanup.push(created.worktreeDir)

  await writeFile(join(created.worktreeDir, "shared.txt"), "worktree dirty\n", "utf-8")
  await writeFile(join(created.worktreeDir, "worktree-note.txt"), "mirror me\n", "utf-8")

  const sessionInput = {
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
    agentBranch: created.branchName,
  }

  const mounted = await mountReviewSession(sessionInput)
  const expectedPrimaryDir = await realpath(repoDir)
  const expectedWorktreeDir = await realpath(created.worktreeDir)
  expect(mounted?.sessionId.startsWith("sha256-")).toBe(true)
  expect(mounted?.reviewWorktree).toBe(expectedPrimaryDir)
  expect(mounted?.agentWorktree).toBe(expectedWorktreeDir)
  expect(mounted?.agentSnapshot).toMatch(/^[0-9a-f]{40}$/)
  expect(mounted?.renderedSnapshot).toMatch(/^[0-9a-f]{40}$/)
  expect(mounted?.reviewBranch).toBe("review-sync/goddard-review-sync-a")
  expect(mounted?.agentBranch).toBe("goddard-review-sync-a")
  expect(await currentBranch(repoDir)).toBe("review-sync/goddard-review-sync-a")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("worktree dirty\n")
  expect(await readFile(join(repoDir, "worktree-note.txt"), "utf-8")).toBe("mirror me\n")

  const found = await findMountedReviewSessionByPrimaryDir(repoDir)
  expect(found?.sessionId).toBe(mounted?.sessionId)

  const rehydrated = await mountReviewSession({ ...sessionInput })
  expect(rehydrated?.sessionId).toBe(mounted?.sessionId)

  const reused = await mountReviewSession(sessionInput)
  expect(reused?.sessionId).toBe(mounted?.sessionId)

  const unmounted = await unmountReviewSession(sessionInput)
  expect(unmounted.warnings).toEqual([])
  expect(await currentBranch(repoDir)).toBe("review-sync/goddard-review-sync-a")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("worktree dirty\n")
  expect(await findMountedReviewSessionByPrimaryDir(repoDir)).toBeNull()

  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
})

test("review session sync reports rejected review patches as warnings", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-review-sync-rejected",
  })
  cleanup.push(created.worktreeDir)

  const sessionInput = {
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
    agentBranch: created.branchName,
  }

  await mountReviewSession(sessionInput)
  await writeFile(join(repoDir, "shared.txt"), "review conflict\n", "utf-8")
  await writeFile(join(created.worktreeDir, "shared.txt"), "agent conflict\n", "utf-8")

  const synced = await syncReviewSessionOnce(sessionInput)
  expect(synced.warnings).toHaveLength(1)
  expect(synced.warnings[0]).toContain("Human patch rejected")
  expect(synced.state.lastSync.status).toBe("rejected-human-patch")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("agent conflict\n")
  expect(await readFile(join(created.worktreeDir, "shared.txt"), "utf-8")).toBe("agent conflict\n")

  await unmountReviewSession(sessionInput)
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
