import { existsSync } from "node:fs"
import { mkdtemp, readdir, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"

import { runCommand } from "./process.ts"

export type WorktreeArchiveFailureCode =
  | "snapshot_ref_write_failed"
  | "restore_path_exists"
  | "worktree_create_failed"
  | "snapshot_apply_failed"
  | "worktree_cleanup_failed"

export class WorktreeArchiveError extends Error {
  constructor(
    public readonly code: WorktreeArchiveFailureCode,
    message: string,
  ) {
    super(message)
    this.name = "WorktreeArchiveError"
  }
}

export type WorktreeArchiveState = {
  status: "archived"
  baseOid: string
  snapshotRef: string | null
  snapshotOid: string | null
  includesIndex: true
  includesUntracked: true
  includesIgnored: false
  archivedAt: string
  originalWorktreeDir: string
}

export type CreateWorktreeArchiveSnapshotParams = {
  sessionId: string
  repoRoot: string
  worktreeDir: string
  snapshotRef?: string
  archivedAt?: string
}

export type RestoreWorktreeArchiveParams = {
  repoRoot: string
  restorePath: string
  archive: WorktreeArchiveState
}

export async function archiveWorktree(
  params: CreateWorktreeArchiveSnapshotParams,
): Promise<WorktreeArchiveState> {
  const archive = await createWorktreeArchiveSnapshot(params)
  await cleanAndRemoveArchivedWorktree({
    repoRoot: params.repoRoot,
    worktreeDir: params.worktreeDir,
  })
  return archive
}

export async function createWorktreeArchiveSnapshot(
  params: CreateWorktreeArchiveSnapshotParams,
): Promise<WorktreeArchiveState> {
  const baseOid = await runGit(params.worktreeDir, ["rev-parse", "--verify", "HEAD"])
  const snapshotRef = params.snapshotRef ?? defaultSnapshotRef(params.sessionId)
  const trackedStashOid = await createTrackedStash(params)
  const untrackedPaths = await listUntrackedNonIgnoredFiles(params.worktreeDir)
  const archivedAt = params.archivedAt ?? new Date().toISOString()

  if (!trackedStashOid && untrackedPaths.length === 0) {
    return {
      status: "archived",
      baseOid,
      snapshotRef: null,
      snapshotOid: null,
      includesIndex: true,
      includesUntracked: true,
      includesIgnored: false,
      archivedAt,
      originalWorktreeDir: params.worktreeDir,
    }
  }

  const snapshotOid =
    untrackedPaths.length === 0
      ? trackedStashOid!
      : await createSnapshotWithUntrackedParent({
          ...params,
          baseOid,
          trackedStashOid,
          untrackedPaths,
        })

  await writeSnapshotRef(params.repoRoot, snapshotRef, snapshotOid)

  return {
    status: "archived",
    baseOid,
    snapshotRef,
    snapshotOid,
    includesIndex: true,
    includesUntracked: true,
    includesIgnored: false,
    archivedAt,
    originalWorktreeDir: params.worktreeDir,
  }
}

export async function cleanAndRemoveArchivedWorktree(params: {
  repoRoot: string
  worktreeDir: string
}): Promise<void> {
  await runGit(params.worktreeDir, ["reset", "--hard", "HEAD"], {
    failureCode: "worktree_cleanup_failed",
  })
  await runGit(params.worktreeDir, ["clean", "-fdx"], {
    failureCode: "worktree_cleanup_failed",
  })
  await runGit(params.repoRoot, ["worktree", "remove", params.worktreeDir], {
    failureCode: "worktree_cleanup_failed",
  })
}

export async function restoreWorktreeArchive(params: RestoreWorktreeArchiveParams): Promise<void> {
  await assertRestorePathAvailable(params.restorePath)
  await runGit(
    params.repoRoot,
    ["worktree", "add", "--detach", params.restorePath, params.archive.baseOid],
    {
      failureCode: "worktree_create_failed",
    },
  )

  if (params.archive.snapshotRef) {
    await runGit(params.restorePath, ["stash", "apply", "--index", params.archive.snapshotRef], {
      failureCode: "snapshot_apply_failed",
    })
  }
}

export function defaultSnapshotRef(sessionId: string): string {
  return `refs/goddard/worktree-archive/${sessionId}/snapshot`
}

async function createTrackedStash(params: CreateWorktreeArchiveSnapshotParams): Promise<string> {
  const result = await runCommand(
    "git",
    ["stash", "create", `goddard archive ${params.sessionId}`],
    {
      cwd: params.worktreeDir,
      stdin: "ignore",
    },
  )
  if (result.status !== 0) {
    throw new WorktreeArchiveError(
      "snapshot_ref_write_failed",
      `Failed to create tracked worktree snapshot: ${result.stderr.trim() || result.stdout.trim()}`,
    )
  }
  return result.stdout.trim()
}

async function createSnapshotWithUntrackedParent(params: {
  sessionId: string
  worktreeDir: string
  baseOid: string
  trackedStashOid: string
  untrackedPaths: string[]
}): Promise<string> {
  const worktreeTree = params.trackedStashOid
    ? await runGit(params.worktreeDir, ["rev-parse", `${params.trackedStashOid}^{tree}`])
    : await runGit(params.worktreeDir, ["rev-parse", `${params.baseOid}^{tree}`])
  const indexCommit = params.trackedStashOid
    ? await runGit(params.worktreeDir, ["rev-parse", `${params.trackedStashOid}^2`])
    : await createIndexCommit(params.worktreeDir, params.baseOid, params.sessionId)
  const untrackedCommit = await createUntrackedFilesCommit(
    params.worktreeDir,
    params.sessionId,
    params.untrackedPaths,
  )

  return await runGit(params.worktreeDir, [
    "commit-tree",
    worktreeTree,
    "-p",
    params.baseOid,
    "-p",
    indexCommit,
    "-p",
    untrackedCommit,
    "-m",
    `goddard archive ${params.sessionId}`,
  ])
}

async function createIndexCommit(
  worktreeDir: string,
  baseOid: string,
  sessionId: string,
): Promise<string> {
  const indexTree = await runGit(worktreeDir, ["write-tree"])
  return await runGit(worktreeDir, [
    "commit-tree",
    indexTree,
    "-p",
    baseOid,
    "-m",
    `index on ${sessionId}`,
  ])
}

async function createUntrackedFilesCommit(
  worktreeDir: string,
  sessionId: string,
  untrackedPaths: string[],
): Promise<string> {
  const tempDir = await mkdtemp(path.join(tmpdir(), "goddard-worktree-archive-index-"))
  const indexPath = path.join(tempDir, "index")
  const env = {
    ...process.env,
    GIT_INDEX_FILE: indexPath,
  }

  try {
    await runGit(worktreeDir, ["read-tree", "--empty"], { env })
    await runGit(worktreeDir, ["add", "--", ...untrackedPaths], { env })
    const untrackedTree = await runGit(worktreeDir, ["write-tree"], { env })
    return await runGit(worktreeDir, [
      "commit-tree",
      untrackedTree,
      "-m",
      `untracked files on ${sessionId}`,
    ])
  } finally {
    await rm(tempDir, { recursive: true, force: true })
  }
}

async function listUntrackedNonIgnoredFiles(worktreeDir: string): Promise<string[]> {
  const output = await runGit(worktreeDir, ["ls-files", "--others", "--exclude-standard", "-z"])
  return output.split("\0").filter((value) => value.length > 0)
}

async function writeSnapshotRef(
  repoRoot: string,
  snapshotRef: string,
  snapshotOid: string,
): Promise<void> {
  await runGit(repoRoot, ["update-ref", "--create-reflog", snapshotRef, snapshotOid], {
    failureCode: "snapshot_ref_write_failed",
  })
  await runGit(repoRoot, ["rev-parse", "--verify", `${snapshotRef}^{commit}`], {
    failureCode: "snapshot_ref_write_failed",
  })
}

async function assertRestorePathAvailable(restorePath: string): Promise<void> {
  if (!existsSync(restorePath)) {
    return
  }

  const entries = await readdir(restorePath)
  if (entries.length > 0) {
    throw new WorktreeArchiveError(
      "restore_path_exists",
      `Restore path already exists and is not empty: ${restorePath}`,
    )
  }
}

async function runGit(
  cwd: string,
  args: string[],
  options: {
    env?: NodeJS.ProcessEnv
    failureCode?: WorktreeArchiveFailureCode
  } = {},
): Promise<string> {
  const result = await runCommand("git", args, {
    cwd,
    env: options.env,
    stdin: "ignore",
  })
  if (result.status !== 0) {
    throw new WorktreeArchiveError(
      options.failureCode ?? "snapshot_ref_write_failed",
      `git ${args.join(" ")} failed: ${result.stderr.trim() || result.stdout.trim()}`,
    )
  }
  return result.stdout.trim()
}
