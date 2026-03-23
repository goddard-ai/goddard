import type { DaemonSessionMetadata } from "@goddard-ai/schema/daemon"
import { Worktree } from "@goddard-ai/worktree"
import { spawnSync } from "node:child_process"
import { realpathSync } from "node:fs"
import { join, relative, resolve } from "node:path"

const defaultWorktreeDirName = ".goddard-agents"

/**
 * Worktree metadata persisted onto daemon sessions so cleanup can be retried later.
 */
export interface SessionWorktreeMetadata {
  repoRoot: string
  requestedCwd: string
  effectiveCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
}

/**
 * Live worktree state owned by one daemon session while its agent process is active.
 */
export interface SessionWorktreeHandle {
  metadata: SessionWorktreeMetadata
  cleanup: () => boolean
}

/**
 * Returns the stored worktree metadata when one was attached to a daemon session.
 */
export function parseSessionWorktreeMetadata(
  metadata: DaemonSessionMetadata | null | undefined,
): SessionWorktreeMetadata | null {
  const candidate =
    metadata && typeof metadata === "object" && "worktree" in metadata ? metadata.worktree : null

  if (!candidate || typeof candidate !== "object") {
    return null
  }

  const { repoRoot, requestedCwd, effectiveCwd, worktreeDir, branchName, poweredBy } =
    candidate as Record<string, unknown>

  if (
    typeof repoRoot !== "string" ||
    typeof requestedCwd !== "string" ||
    typeof effectiveCwd !== "string" ||
    typeof worktreeDir !== "string" ||
    typeof branchName !== "string" ||
    typeof poweredBy !== "string"
  ) {
    return null
  }

  return {
    repoRoot,
    requestedCwd,
    effectiveCwd,
    worktreeDir,
    branchName,
    poweredBy,
  }
}

/**
 * Creates one daemon-owned worktree and maps the requested cwd into the cloned workspace.
 */
export function createSessionWorktree(
  sessionId: string,
  cwd: string,
  metadata?: DaemonSessionMetadata,
): SessionWorktreeHandle | null {
  const requestedCwd = resolveRealPath(cwd)
  const repoRoot = resolveGitRepoRoot(requestedCwd)
  if (!repoRoot) {
    return null
  }

  const relativeCwd = relative(repoRoot, requestedCwd)
  const branchName =
    typeof metadata?.prNumber === "number" ? `pr-${metadata.prNumber}` : `session-${sessionId}`
  const worktree = new Worktree({
    cwd: repoRoot,
    defaultPluginDirName: defaultWorktreeDirName,
  })
  const { worktreeDir } = worktree.setup(branchName)
  const effectiveCwd =
    relativeCwd.length === 0 ? worktreeDir : join(worktreeDir, normalizeRelativePath(relativeCwd))

  let cleaned = false

  return {
    metadata: {
      repoRoot,
      requestedCwd,
      effectiveCwd,
      worktreeDir,
      branchName,
      poweredBy: worktree.poweredBy,
    },
    cleanup: () => {
      if (cleaned) {
        return true
      }

      cleaned = true
      return worktree.cleanup(worktreeDir, branchName)
    },
  }
}

/**
 * Removes one persisted daemon worktree using metadata recorded on the session.
 */
export function cleanupSessionWorktree(metadata: SessionWorktreeMetadata): boolean {
  const worktree = new Worktree({
    cwd: metadata.repoRoot,
    defaultPluginDirName: defaultWorktreeDirName,
  })
  return worktree.cleanup(metadata.worktreeDir, metadata.branchName)
}

/**
 * Resolves the containing git repository root for one requested session cwd when one exists.
 */
function resolveGitRepoRoot(cwd: string): string | null {
  const result = spawnSync("git", ["rev-parse", "--show-toplevel"], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  })

  if (result.status !== 0) {
    return null
  }

  const repoRoot = result.stdout.toString("utf8").trim()
  if (!repoRoot) {
    return null
  }

  return resolve(repoRoot)
}

/**
 * Normalizes a relative path before it is appended under a worktree root.
 */
function normalizeRelativePath(value: string): string {
  return value === "." ? "" : value
}

/**
 * Resolves one path through the filesystem so symlinked temp roots stay consistent.
 */
function resolveRealPath(value: string): string {
  return resolve(realpathSync.native(value))
}
