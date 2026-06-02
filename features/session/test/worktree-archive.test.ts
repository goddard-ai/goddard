import { existsSync, realpathSync } from "node:fs"
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  archiveWorktree,
  createWorktreeArchiveSnapshot,
  defaultSnapshotRef,
  restoreWorktreeArchive,
  WorktreeArchiveError,
  type WorktreeArchiveState,
} from "../src/daemon/worktrees/archive.ts"
import { runCommand } from "../src/daemon/worktrees/process.ts"

const cleanup: string[] = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

test("archives and restores tracked and untracked non-ignored files without creating refs/stash or branches", async () => {
  const repoRoot = await createRepoFixture()
  const worktreeDir = await createLinkedWorktree(repoRoot, "archive-success")
  const branchCountBefore = await branchCount(repoRoot)

  await writeFile(path.join(worktreeDir, "tracked.txt"), "tracked archive change\n", "utf8")
  await runGit(worktreeDir, ["add", "tracked.txt"])
  await writeFile(path.join(worktreeDir, "untracked.txt"), "untracked archive file\n", "utf8")
  await writeFile(path.join(worktreeDir, "ignored.log"), "ignored archive file\n", "utf8")

  const archive = await archiveWorktree({
    sessionId: "ses_archive_success",
    repoRoot,
    worktreeDir,
    archivedAt: "2026-06-02T00:00:00.000Z",
  })

  expect(archive.snapshotRef).toBe(defaultSnapshotRef("ses_archive_success"))
  expect(archive.snapshotOid).toBeTruthy()
  expect(archive.includesUntracked).toBe(true)
  expect(archive.includesIgnored).toBe(false)
  expect(existsSync(worktreeDir)).toBe(false)
  expect(await revParseStatus(repoRoot, ["--verify", "refs/stash"])).toBe(128)
  expect(await branchCount(repoRoot)).toBe(branchCountBefore)

  await restoreWorktreeArchive({ repoRoot, restorePath: worktreeDir, archive })

  expect(await readFile(path.join(worktreeDir, "tracked.txt"), "utf8")).toBe(
    "tracked archive change\n",
  )
  expect(await readFile(path.join(worktreeDir, "untracked.txt"), "utf8")).toBe(
    "untracked archive file\n",
  )
  expect(existsSync(path.join(worktreeDir, "ignored.log"))).toBe(false)
  expect(await gitStatus(worktreeDir, ["symbolic-ref", "-q", "HEAD"])).toBe(1)
  expect(await runGit(worktreeDir, ["diff", "--cached", "--name-only"])).toBe("tracked.txt")
})

test("clean worktree archive records no snapshot ref", async () => {
  const repoRoot = await createRepoFixture()
  const worktreeDir = await createLinkedWorktree(repoRoot, "archive-clean")

  const archive = await createWorktreeArchiveSnapshot({
    sessionId: "ses_archive_clean",
    repoRoot,
    worktreeDir,
    archivedAt: "2026-06-02T00:00:00.000Z",
  })

  expect(archive.snapshotRef).toBe(null)
  expect(archive.snapshotOid).toBe(null)
  expect(
    await revParseStatus(repoRoot, ["--verify", defaultSnapshotRef("ses_archive_clean")]),
  ).toBe(128)
})

test("untracked-only archive restores non-ignored files without preserving ignored files", async () => {
  const repoRoot = await createRepoFixture()
  const worktreeDir = await createLinkedWorktree(repoRoot, "archive-untracked-only")
  await writeFile(path.join(worktreeDir, "untracked-only.txt"), "untracked only\n", "utf8")
  await writeFile(path.join(worktreeDir, "ignored.log"), "ignored only\n", "utf8")

  const archive = await archiveWorktree({
    sessionId: "ses_archive_untracked_only",
    repoRoot,
    worktreeDir,
  })

  expect(archive.snapshotRef).toBe(defaultSnapshotRef("ses_archive_untracked_only"))
  expect(archive.snapshotOid).toBeTruthy()
  expect(existsSync(worktreeDir)).toBe(false)

  await restoreWorktreeArchive({ repoRoot, restorePath: worktreeDir, archive })

  expect(await readFile(path.join(worktreeDir, "untracked-only.txt"), "utf8")).toBe(
    "untracked only\n",
  )
  expect(existsSync(path.join(worktreeDir, "ignored.log"))).toBe(false)
})

test("private ref write failure leaves the source worktree untouched", async () => {
  const repoRoot = await createRepoFixture()
  const worktreeDir = await createLinkedWorktree(repoRoot, "archive-ref-failure")
  await writeFile(path.join(worktreeDir, "tracked.txt"), "must remain\n", "utf8")

  await expect(
    archiveWorktree({
      sessionId: "ses_archive_ref_failure",
      repoRoot,
      worktreeDir,
      snapshotRef: "refs/goddard/worktree archive/invalid",
    }),
  ).rejects.toMatchObject({ code: "snapshot_ref_write_failed" })

  expect(existsSync(worktreeDir)).toBe(true)
  expect(await readFile(path.join(worktreeDir, "tracked.txt"), "utf8")).toBe("must remain\n")
})

test("restore fails before worktree creation when the target path is occupied", async () => {
  const repoRoot = await createRepoFixture()
  const restorePath = await mkdtemp(path.join(tmpdir(), "goddard-archive-occupied-"))
  cleanup.push(restorePath)
  await writeFile(path.join(restorePath, "file.txt"), "occupied\n", "utf8")

  await expect(
    restoreWorktreeArchive({
      repoRoot,
      restorePath,
      archive: cleanArchive(await runGit(repoRoot, ["rev-parse", "HEAD"]), restorePath),
    }),
  ).rejects.toMatchObject({ code: "restore_path_exists" })
})

test("snapshot apply failure keeps the partially restored worktree for inspection", async () => {
  const repoRoot = await createRepoFixture()
  const restorePath = path.join(await mkdtemp(path.join(tmpdir(), "goddard-archive-apply-")), "wt")
  cleanup.push(path.dirname(restorePath))

  await expect(
    restoreWorktreeArchive({
      repoRoot,
      restorePath,
      archive: {
        ...cleanArchive(await runGit(repoRoot, ["rev-parse", "HEAD"]), restorePath),
        snapshotRef: "refs/goddard/worktree-archive/missing/snapshot",
        snapshotOid: "missing",
      },
    }),
  ).rejects.toMatchObject({ code: "snapshot_apply_failed" })

  expect(existsSync(restorePath)).toBe(true)
})

async function createRepoFixture() {
  const repoRoot = realpathSync.native(await mkdtemp(path.join(tmpdir(), "goddard-archive-repo-")))
  cleanup.push(repoRoot)

  await writeFile(path.join(repoRoot, ".gitignore"), "*.log\n", "utf8")
  await writeFile(path.join(repoRoot, "tracked.txt"), "tracked base\n", "utf8")
  await runGit(repoRoot, ["init"])
  await runGit(repoRoot, ["config", "user.email", "bot@example.com"])
  await runGit(repoRoot, ["config", "user.name", "Bot"])
  await runGit(repoRoot, ["add", "."])
  await runGit(repoRoot, ["commit", "-m", "init"])

  return repoRoot
}

async function createLinkedWorktree(repoRoot: string, name: string) {
  const parent = await mkdtemp(path.join(tmpdir(), "goddard-archive-worktree-"))
  cleanup.push(parent)
  const worktreeDir = path.join(parent, name)
  await runGit(repoRoot, ["worktree", "add", "--detach", worktreeDir, "HEAD"])
  return realpathSync.native(worktreeDir)
}

function cleanArchive(baseOid: string, worktreeDir: string): WorktreeArchiveState {
  return {
    status: "archived",
    baseOid,
    snapshotRef: null,
    snapshotOid: null,
    includesIndex: true,
    includesUntracked: true,
    includesIgnored: false,
    archivedAt: "2026-06-02T00:00:00.000Z",
    originalWorktreeDir: worktreeDir,
  }
}

async function branchCount(repoRoot: string) {
  const output = await runGit(repoRoot, ["for-each-ref", "--format=%(refname)", "refs/heads"])
  return output.length === 0 ? 0 : output.split("\n").length
}

async function revParseStatus(cwd: string, args: string[]) {
  return await gitStatus(cwd, ["rev-parse", ...args])
}

async function gitStatus(cwd: string, args: string[]) {
  const result = await runCommand("git", args, {
    cwd,
    stdin: "ignore",
  })
  return result.status
}

async function runGit(cwd: string, args: string[]) {
  const result = await runCommand("git", args, {
    cwd,
    stdin: "ignore",
  })
  if (result.status !== 0) {
    throw new WorktreeArchiveError(
      "snapshot_ref_write_failed",
      `git ${args.join(" ")} failed: ${result.stderr.trim() || result.stdout.trim()}`,
    )
  }
  return result.stdout.trim()
}
