import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"
import type * as acp from "acp-client/protocol"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import type {
  ArchiveSessionRequest,
  MutateSessionArchiveResponse,
  SessionIdParams,
  UnarchiveSessionRequest,
} from "./schema.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
  wrap({ client }) {
    return {
      session: {
        /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
        subscribe: async (
          input: SessionIdParams,
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
        /** Archives one daemon-managed session and removes its worktree when supported by the daemon. */
        archive: async (input: ArchiveSessionRequest): Promise<MutateSessionArchiveResponse> =>
          client.session.archive(input),
        /** Unarchives one daemon-managed session and restores its worktree when supported by the daemon. */
        unarchive: async (input: UnarchiveSessionRequest): Promise<MutateSessionArchiveResponse> =>
          client.session.unarchive(input),
      },
    }
  },
})
