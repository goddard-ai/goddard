import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { sessionIpcSchema } from "./daemon-ipc.ts"
import type {
  GetSessionWorktreeRequest,
  MountSessionReviewSessionRequest,
  RunSessionReviewSessionRequest,
  UnmountSessionReviewSessionRequest,
} from "./schema.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipc: sessionIpcSchema,
  create({ client }) {
    return {
      session: {
        /** Reads persisted worktree metadata attached to one daemon-managed session. */
        worktree: async (input: GetSessionWorktreeRequest) =>
          client.send("session.worktree.get", input),

        /** Mounts a review session for one daemon-managed session worktree. */
        mountReviewSession: async (input: MountSessionReviewSessionRequest) =>
          client.send("session.reviewSession.mount", input),

        /** Runs one mounted review session immediately. */
        runReviewSession: async (input: RunSessionReviewSessionRequest) =>
          client.send("session.reviewSession.run", input),

        /** Unmounts a review session from one daemon-managed session worktree. */
        unmountReviewSession: async (input: UnmountSessionReviewSessionRequest) =>
          client.send("session.reviewSession.unmount", input),
      },
    }
  },
})
