import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import type {
  GetSessionWorktreeRequest,
  MountSessionReviewSessionRequest,
  RunSessionReviewSessionRequest,
  UnmountSessionReviewSessionRequest,
} from "./schema.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
  create({ client }) {
    return {
      session: {
        /** Reads persisted worktree metadata attached to one daemon-managed session. */
        worktree: (input: GetSessionWorktreeRequest) => client.send("session.worktree.get", input),

        /** Mounts a review session for one daemon-managed session worktree. */
        mountReviewSession: (input: MountSessionReviewSessionRequest) =>
          client.send("session.reviewSession.mount", input),

        /** Runs one mounted review session immediately. */
        runReviewSession: (input: RunSessionReviewSessionRequest) =>
          client.send("session.reviewSession.run", input),

        /** Unmounts a review session from one daemon-managed session worktree. */
        unmountReviewSession: (input: UnmountSessionReviewSessionRequest) =>
          client.send("session.reviewSession.unmount", input),
      },
    }
  },
})
