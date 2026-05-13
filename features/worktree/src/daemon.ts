import { definePlugin } from "@goddard-ai/daemon-plugin"

import { worktreeIpcSchema } from "./daemon-ipc.ts"
import type {
  GetSessionWorktreeRequest,
  GetSessionWorktreeResponse,
  MountSessionReviewSessionRequest,
  MutateSessionReviewSessionResponse,
  RunSessionReviewSessionRequest,
  SessionWorktreeIdentity,
  UnmountSessionReviewSessionRequest,
} from "./schema.ts"

/** Session worktree behavior exposed for the session feature and daemon IPC handlers. */
export interface WorktreeFeatureExtension {
  getWorktree: (id: SessionWorktreeIdentity["id"]) => Promise<GetSessionWorktreeResponse>
  mountReviewSession: (
    id: SessionWorktreeIdentity["id"],
  ) => Promise<MutateSessionReviewSessionResponse>
  runReviewSession: (
    id: SessionWorktreeIdentity["id"],
  ) => Promise<MutateSessionReviewSessionResponse>
  unmountReviewSession: (
    id: SessionWorktreeIdentity["id"],
  ) => Promise<MutateSessionReviewSessionResponse>
}

export const worktreePlugin = definePlugin({
  name: "worktree",
  ipc: worktreeIpcSchema,
  provides: {
    worktree: null as unknown as WorktreeFeatureExtension,
  },
  createRequestHandlers(context: WorktreeFeatureExtension) {
    return {
      "session.worktree.get": async ({ id }: GetSessionWorktreeRequest) => context.getWorktree(id),
      "session.reviewSession.mount": async ({ id }: MountSessionReviewSessionRequest) =>
        context.mountReviewSession(id),
      "session.reviewSession.run": async ({ id }: RunSessionReviewSessionRequest) =>
        context.runReviewSession(id),
      "session.reviewSession.unmount": async ({ id }: UnmountSessionReviewSessionRequest) =>
        context.unmountReviewSession(id),
    }
  },
})
