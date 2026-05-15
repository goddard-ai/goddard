import { defineRequest, defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { sessionIpcSchema } from "./daemon-ipc.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipc: sessionIpcSchema,
  create({ client }) {
    return {
      session: {
        /** Reads persisted worktree metadata attached to one daemon-managed session. */
        worktree: defineRequest(client, "session.worktree.get"),

        /** Mounts a review session for one daemon-managed session worktree. */
        mountReviewSession: defineRequest(client, "session.reviewSession.mount"),

        /** Runs one mounted review session immediately. */
        runReviewSession: defineRequest(client, "session.reviewSession.run"),

        /** Unmounts a review session from one daemon-managed session worktree. */
        unmountReviewSession: defineRequest(client, "session.reviewSession.unmount"),
      },
    }
  },
})
