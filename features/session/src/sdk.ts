import type * as acp from "@agentclientprotocol/sdk"
import type { DaemonSessionIdParams } from "@goddard-ai/schema/id"
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
        worktree: (input: GetSessionWorktreeRequest) => client.session.worktree.get(input),

        /** Mounts a review session for one daemon-managed session worktree. */
        mountReviewSession: (input: MountSessionReviewSessionRequest) =>
          client.session.reviewSession.mount(input),

        /** Runs one mounted review session immediately. */
        runReviewSession: (input: RunSessionReviewSessionRequest) =>
          client.session.reviewSession.run(input),

        /** Unmounts a review session from one daemon-managed session worktree. */
        unmountReviewSession: (input: UnmountSessionReviewSessionRequest) =>
          client.session.reviewSession.unmount(input),

        /** Reads persisted workforce metadata attached to one daemon-managed session. */
        workforce: async (input: DaemonSessionIdParams) => client.session.workforce.get(input),

        /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
        subscribe: async (
          input: DaemonSessionIdParams,
          onMessage: (message: acp.AnyMessage) => void,
        ): Promise<() => void> => {
          const controller = new AbortController()
          const events = await client.session.messageEvents(input, { signal: controller.signal })
          void (async () => {
            for await (const payload of events) {
              if (controller.signal.aborted) {
                break
              }
              onMessage(payload.message)
            }
          })()
          return () => controller.abort()
        },
      },
    }
  },
})
