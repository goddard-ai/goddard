/** Git-backed helpers for capturing and materializing daemon session turn file changes. */
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import type {
  DaemonSessionTurnChange,
  DaemonSessionTurnChangedFile,
  SessionId,
} from "../schema.ts"
import { resolveGitRepoRoot } from "./worktree.ts"

/** One git baseline captured when the daemon dispatches a prompt turn. */
export type SessionTurnGitBaseline = {
  repoRoot: string
  startedDirty: boolean
  treeOid: string
}

/** One materialized git diff artifact produced after a prompt turn completes. */
export type SessionTurnGitChangeArtifact = {
  repoRoot: string
  startedDirty: boolean
  warnings: string[]
  changedFiles: DaemonSessionTurnChangedFile[]
  patch: string
}

type GitCommandResult = {
  success: boolean
  stdout: string
  stderr: string
}

async function readSpawnStream(stream: number | ReadableStream<Uint8Array> | null | undefined) {
  return stream && typeof stream !== "number" ? new Response(stream as BodyInit).text() : ""
}

/** Runs one git subprocess inside the target repository root. */
async function runGit(cwd: string, args: string[], env: Record<string, string | undefined> = {}) {
  let result: ReturnType<typeof Bun.spawn>
  try {
    result = Bun.spawn(["git", ...args], {
      cwd,
      env: {
        ...process.env,
        ...env,
      },
      stdin: "ignore",
      stdout: "pipe",
      stderr: "pipe",
    })
  } catch (error) {
    return {
      success: false,
      stdout: "",
      stderr: error instanceof Error ? error.message : String(error),
    } satisfies GitCommandResult
  }

  const [stdout, stderr] = await Promise.all([
    readSpawnStream(result.stdout),
    readSpawnStream(result.stderr),
  ])
  await result.exited

  return {
    success: result.exitCode === 0,
    stdout,
    stderr,
  } satisfies GitCommandResult
}

/** Writes one git tree object that reflects the current worktree without mutating the real index. */
async function writeWorktreeSnapshotTree(repoRoot: string) {
  const statusResult = await runGit(repoRoot, ["status", "--porcelain", "-z"])
  if (!statusResult.success) {
    return null
  }

  const changedPaths = [...new Set(parseStatusPaths(statusResult.stdout))]
  const tempDir = await mkdtemp(join(tmpdir(), "goddard-turn-git-"))
  const indexPath = join(tempDir, "index")
  const gitIndexEnv = {
    GIT_INDEX_FILE: indexPath,
  }

  try {
    const headCheck = await runGit(repoRoot, ["rev-parse", "--verify", "HEAD"])
    if (headCheck.success) {
      const readTreeResult = await runGit(repoRoot, ["read-tree", "HEAD"], gitIndexEnv)
      if (!readTreeResult.success) {
        return null
      }
    }

    if (changedPaths.length > 0) {
      const addResult = await runGit(repoRoot, ["add", "-A", "--", ...changedPaths], gitIndexEnv)
      if (!addResult.success) {
        return null
      }
    }

    const writeTreeResult = await runGit(repoRoot, ["write-tree"], gitIndexEnv)
    if (!writeTreeResult.success) {
      return null
    }

    const treeOid = writeTreeResult.stdout.trim()
    return treeOid.length > 0 ? treeOid : null
  } finally {
    await rm(tempDir, { recursive: true, force: true }).catch(() => {})
  }
}

function parseStatusPaths(output: string) {
  const tokens = output.split("\0").filter((token) => token.length > 0)
  const paths: string[] = []

  for (let index = 0; index < tokens.length; ) {
    const entry = tokens[index++] ?? ""
    if (entry.length < 4) {
      continue
    }

    const statusToken = entry.slice(0, 2)
    const path = entry.slice(3)
    if (path.length > 0) {
      paths.push(path)
    }

    if (statusToken.includes("R") || statusToken.includes("C")) {
      const previousPath = tokens[index++] ?? ""
      if (previousPath.length > 0) {
        paths.push(previousPath)
      }
    }
  }

  return paths
}

function toChangedFileStatus(statusToken: string): DaemonSessionTurnChangedFile["status"] {
  const statusPrefix = statusToken.slice(0, 1)

  if (statusPrefix === "A") {
    return "added"
  }

  if (statusPrefix === "M") {
    return "modified"
  }

  if (statusPrefix === "D") {
    return "deleted"
  }

  if (statusPrefix === "R") {
    return "renamed"
  }

  if (statusPrefix === "C") {
    return "copied"
  }

  if (statusPrefix === "T") {
    return "type_changed"
  }

  return "unknown"
}

/** Parses one `git diff --name-status -z` payload into file-level change records. */
function parseChangedFiles(output: string) {
  const tokens = output.split("\0").filter((token) => token.length > 0)
  const changedFiles: DaemonSessionTurnChangedFile[] = []

  for (let index = 0; index < tokens.length; ) {
    const statusToken = tokens[index++] ?? ""
    const path = tokens[index++] ?? ""
    if (!statusToken || !path) {
      break
    }

    const status = toChangedFileStatus(statusToken)
    if (status === "renamed" || status === "copied") {
      const nextPath = tokens[index++] ?? ""
      if (!nextPath) {
        break
      }

      changedFiles.push({
        path: nextPath,
        previousPath: path,
        status,
      })
      continue
    }

    changedFiles.push({
      path,
      previousPath: null,
      status,
    })
  }

  return changedFiles
}

/** Captures one git baseline for the current repository worktree before a prompt turn runs. */
export async function captureSessionTurnGitBaseline(cwd: string) {
  const repoRoot = await resolveGitRepoRoot(cwd)
  if (!repoRoot) {
    return null
  }

  const statusResult = await runGit(repoRoot, ["status", "--porcelain"])
  if (!statusResult.success) {
    return null
  }

  const treeOid = await writeWorktreeSnapshotTree(repoRoot)
  if (!treeOid) {
    return null
  }

  return {
    repoRoot,
    startedDirty: statusResult.stdout.trim().length > 0,
    treeOid,
  } satisfies SessionTurnGitBaseline
}

/** Builds the finalized git patch and changed-file list for one completed prompt turn. */
export async function buildSessionTurnGitChangeArtifact(baseline: SessionTurnGitBaseline) {
  const afterTreeOid = await writeWorktreeSnapshotTree(baseline.repoRoot)
  if (!afterTreeOid) {
    return null
  }

  const [patchResult, changedFilesResult] = await Promise.all([
    runGit(baseline.repoRoot, [
      "diff",
      "--find-renames",
      "--binary",
      baseline.treeOid,
      afterTreeOid,
    ]),
    runGit(baseline.repoRoot, [
      "diff",
      "--name-status",
      "-z",
      "--find-renames",
      baseline.treeOid,
      afterTreeOid,
    ]),
  ])

  if (!patchResult.success || !changedFilesResult.success) {
    return null
  }

  return {
    repoRoot: baseline.repoRoot,
    startedDirty: baseline.startedDirty,
    warnings: [],
    changedFiles: parseChangedFiles(changedFilesResult.stdout),
    patch: patchResult.stdout,
  } satisfies SessionTurnGitChangeArtifact
}

/** Builds one durable turn-change record when a completed turn produced a git patch. */
export async function buildSessionTurnChangeRecord(params: {
  sessionId: SessionId
  turnId: string
  sequence: number
  promptRequestId: string | number
  startedAt: string
  completedAt: string | null
  baseline: SessionTurnGitBaseline | null
}): Promise<Omit<DaemonSessionTurnChange, "id"> | null> {
  if (!params.baseline || !params.completedAt) {
    return null
  }

  const artifact = await buildSessionTurnGitChangeArtifact(params.baseline)
  if (!artifact || artifact.changedFiles.length === 0 || artifact.patch.trim().length === 0) {
    return null
  }

  return {
    sessionId: params.sessionId,
    turnId: params.turnId,
    sequence: params.sequence,
    promptRequestId: params.promptRequestId,
    repoRoot: artifact.repoRoot,
    startedAt: params.startedAt,
    completedAt: params.completedAt,
    startedDirty: artifact.startedDirty,
    warnings: artifact.warnings,
    changedFiles: artifact.changedFiles,
    patch: artifact.patch,
  }
}
