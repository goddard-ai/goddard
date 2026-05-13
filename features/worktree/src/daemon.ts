import { definePlugin, defineSetupContext } from "@goddard-ai/daemon-plugin"

import { worktreeIpcSchema } from "./daemon-ipc.ts"
import type {
  GetSessionWorktreeResponse,
  MutateSessionReviewSessionResponse,
  SessionWorktreeIdentity,
} from "./schema.ts"

export const worktreePlugin = definePlugin({
  name: "worktree",
  ipc: worktreeIpcSchema,
  setupContext: defineSetupContext<{
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
  }>(),
  setup(context) {
    const worktree = {
      getWorktree: context.getWorktree,
      mountReviewSession: context.mountReviewSession,
      runReviewSession: context.runReviewSession,
      unmountReviewSession: context.unmountReviewSession,
    }

    return {
      provides: {
        worktree,
      },
      requestHandlers: {
        "session.worktree.get": async ({ id }) => worktree.getWorktree(id),
        "session.reviewSession.mount": async ({ id }) => worktree.mountReviewSession(id),
        "session.reviewSession.run": async ({ id }) => worktree.runReviewSession(id),
        "session.reviewSession.unmount": async ({ id }) => worktree.unmountReviewSession(id),
      },
    }
  },
})
