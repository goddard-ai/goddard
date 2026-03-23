import type { DaemonSessionMetadata } from "@goddard-ai/schema/daemon"
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
export declare function parseSessionWorktreeMetadata(
  metadata: DaemonSessionMetadata | null | undefined,
): SessionWorktreeMetadata | null
/**
 * Creates one daemon-owned worktree and maps the requested cwd into the cloned workspace.
 */
export declare function createSessionWorktree(
  sessionId: string,
  cwd: string,
  metadata?: DaemonSessionMetadata,
): SessionWorktreeHandle | null
/**
 * Removes one persisted daemon worktree using metadata recorded on the session.
 */
export declare function cleanupSessionWorktree(metadata: SessionWorktreeMetadata): boolean
//# sourceMappingURL=worktree.d.ts.map
