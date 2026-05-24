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
  wrap({ client }) {
    return {
      session: {
        /** Reads persisted worktree metadata attached to one daemon-managed session. */
        worktree: (input: GetSessionWorktreeRequest) =>
          client.session.worktree.get({ body: input }),

        /** Mounts a review session for one daemon-managed session worktree. */
        mountReviewSession: (input: MountSessionReviewSessionRequest) =>
          client.session.reviewSession.mount({ body: input }),

        /** Runs one mounted review session immediately. */
        runReviewSession: (input: RunSessionReviewSessionRequest) =>
          client.session.reviewSession.run({ body: input }),

        /** Unmounts a review session from one daemon-managed session worktree. */
        unmountReviewSession: (input: UnmountSessionReviewSessionRequest) =>
          client.session.reviewSession.unmount({ body: input }),
      },
    }
  },
})
