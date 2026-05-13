import { definePlugin, defineSetupContext } from "@goddard-ai/daemon-plugin"

import { sessionIpcSchema } from "./daemon-ipc.ts"
import type {
  GetSessionWorktreeResponse,
  MutateSessionReviewSessionResponse,
  SessionWorktreeIdentity,
} from "./schema.ts"

type SessionHandlerContext = {
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

export const sessionPlugin = definePlugin({
  name: "session",
  ipc: sessionIpcSchema,
  setupContext: defineSetupContext<SessionHandlerContext>(),
  setup(context) {
    const session = {
      getWorktree: context.getWorktree,
      mountReviewSession: context.mountReviewSession,
      runReviewSession: context.runReviewSession,
      unmountReviewSession: context.unmountReviewSession,
    }

    return {
      provides: {
        session,
      },
      requestHandlers: {
        "session.worktree.get": async ({ id }) => session.getWorktree(id),
        "session.reviewSession.mount": async ({ id }) => session.mountReviewSession(id),
        "session.reviewSession.run": async ({ id }) => session.runReviewSession(id),
        "session.reviewSession.unmount": async ({ id }) => session.unmountReviewSession(id),
      },
    }
  },
})
