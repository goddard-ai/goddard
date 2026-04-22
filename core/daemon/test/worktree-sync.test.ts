import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { createWorktree, deleteWorktree } from "../src/worktrees/index.ts"
import { WorktreeSyncSessionHost } from "../src/worktrees/sync.ts"

const cleanup: string[] = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("mount sync mirrors the worktree and unmount restores primary pre-mount state", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-sync-a",
  })
  cleanup.push(created.worktreeDir)

  await writeFile(join(repoDir, "shared.txt"), "primary pre-mount\n", "utf-8")
  await writeFile(join(repoDir, "primary-note.txt"), "keep me\n", "utf-8")
  await writeFile(join(created.worktreeDir, "shared.txt"), "worktree dirty\n", "utf-8")
  await writeFile(join(created.worktreeDir, "worktree-note.txt"), "mirror me\n", "utf-8")

  const host = new WorktreeSyncSessionHost({
    sessionId: "ses_mount_restore",
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
  })

  const mounted = await host.mount()
  expect(mounted.status).toBe("mounted")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("worktree dirty\n")
  expect(await readFile(join(repoDir, "worktree-note.txt"), "utf-8")).toBe("mirror me\n")
  expect(await readFile(join(created.worktreeDir, "shared.txt"), "utf-8")).toBe("worktree dirty\n")

  const unmounted = await host.unmount()
  expect(unmounted.warnings).toEqual([])
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("primary pre-mount\n")
  expect(await readFile(join(repoDir, "primary-note.txt"), "utf-8")).toBe("keep me\n")

  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
})

test("syncOnce mirrors one-sided untracked files from the worktree to the primary checkout", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-sync-untracked",
  })
  cleanup.push(created.worktreeDir)

  const host = new WorktreeSyncSessionHost({
    sessionId: "ses_untracked_one_sided",
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
  })

  await host.mount()
  await writeFile(join(created.worktreeDir, "new-file.ts"), "export const value = 1\n", "utf-8")

  const synced = await host.syncOnce()
  expect(synced.warnings).toEqual([])
  expect(await readFile(join(repoDir, "new-file.ts"), "utf-8")).toBe("export const value = 1\n")
  expect(await readFile(join(created.worktreeDir, "new-file.ts"), "utf-8")).toBe(
    "export const value = 1\n",
  )

  await host.unmount()
  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
})

test("syncOnce preserves non-conflicting changes and prefers the worktree on conflicts", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
    "primary-only.txt": "base primary\n",
    "worktree-only.txt": "base worktree\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-sync-b",
  })
  cleanup.push(created.worktreeDir)

  const host = new WorktreeSyncSessionHost({
    sessionId: "ses_merge",
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
  })

  await host.mount()
  await writeFile(join(repoDir, "primary-only.txt"), "primary changed\n", "utf-8")
  await writeFile(join(repoDir, "shared.txt"), "primary conflict\n", "utf-8")
  await writeFile(join(created.worktreeDir, "worktree-only.txt"), "worktree changed\n", "utf-8")
  await writeFile(join(created.worktreeDir, "shared.txt"), "worktree conflict\n", "utf-8")

  const synced = await host.syncOnce()
  expect(synced.warnings.length).toBeGreaterThanOrEqual(0)
  expect(await readFile(join(repoDir, "primary-only.txt"), "utf-8")).toBe("primary changed\n")
  expect(await readFile(join(repoDir, "worktree-only.txt"), "utf-8")).toBe("worktree changed\n")
  expect(await readFile(join(repoDir, "shared.txt"), "utf-8")).toBe("worktree conflict\n")
  expect(await readFile(join(created.worktreeDir, "primary-only.txt"), "utf-8")).toBe(
    "primary changed\n",
  )
  expect(await readFile(join(created.worktreeDir, "worktree-only.txt"), "utf-8")).toBe(
    "worktree changed\n",
  )
  expect(await readFile(join(created.worktreeDir, "shared.txt"), "utf-8")).toBe(
    "worktree conflict\n",
  )

  await host.unmount()
  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
})

test("syncOnce keeps distinct untracked files from both sides and prefers the worktree on add/add conflicts", async () => {
  const repoDir = await createRepoFixture({
    "shared.txt": "base\n",
  })
  const created = await createWorktree({
    cwd: repoDir,
    branchName: "goddard-sync-untracked-merge",
  })
  cleanup.push(created.worktreeDir)

  const host = new WorktreeSyncSessionHost({
    sessionId: "ses_untracked_merge",
    primaryDir: repoDir,
    worktreeDir: created.worktreeDir,
  })

  await host.mount()
  await writeFile(join(repoDir, "primary-only.ts"), "export const primary = true\n", "utf-8")
  await writeFile(join(repoDir, "conflict.ts"), "export const source = 'primary'\n", "utf-8")
  await writeFile(
    join(created.worktreeDir, "worktree-only.ts"),
    "export const worktree = true\n",
    "utf-8",
  )
  await writeFile(
    join(created.worktreeDir, "conflict.ts"),
    "export const source = 'worktree'\n",
    "utf-8",
  )

  const synced = await host.syncOnce()
  expect(synced.warnings).toEqual([])
  expect(await readFile(join(repoDir, "primary-only.ts"), "utf-8")).toBe(
    "export const primary = true\n",
  )
  expect(await readFile(join(repoDir, "worktree-only.ts"), "utf-8")).toBe(
    "export const worktree = true\n",
  )
  expect(await readFile(join(repoDir, "conflict.ts"), "utf-8")).toBe(
    "export const source = 'worktree'\n",
  )
  expect(await readFile(join(created.worktreeDir, "primary-only.ts"), "utf-8")).toBe(
    "export const primary = true\n",
  )
  expect(await readFile(join(created.worktreeDir, "worktree-only.ts"), "utf-8")).toBe(
    "export const worktree = true\n",
  )
  expect(await readFile(join(created.worktreeDir, "conflict.ts"), "utf-8")).toBe(
    "export const source = 'worktree'\n",
  )

  await host.unmount()
  await deleteWorktree({
    cwd: repoDir,
    worktreeDir: created.worktreeDir,
    branchName: created.branchName,
    poweredBy: created.poweredBy,
  })
})

async function createRepoFixture(files: Record<string, string>) {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-worktree-sync-repo-"))
  cleanup.push(repoDir)

  for (const [relativePath, content] of Object.entries(files)) {
    await writeFile(join(repoDir, relativePath), content, "utf-8")
  }

  await runGit(repoDir, ["init"])
  await runGit(repoDir, ["config", "user.email", "bot@example.com"])
  await runGit(repoDir, ["config", "user.name", "Bot"])
  await runGit(repoDir, ["add", "."])
  await runGit(repoDir, ["commit", "-m", "init"])

  expect(existsSync(repoDir)).toBe(true)
  return repoDir
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
}
