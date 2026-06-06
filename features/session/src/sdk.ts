import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"
import type * as acp from "acp-client/protocol"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import type { SessionIdParams, SessionLifecycleEvent } from "./schema.ts"

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
        lifecycle: {
          /** Subscribes to app-wide daemon session lifecycle updates. */
          subscribe: async (
            onEvent: (event: SessionLifecycleEvent) => void,
          ): Promise<() => void> => {
            const controller = new AbortController()
            const events = await client.session.lifecycleEvents(undefined, {
              signal: controller.signal,
            })
            void (async () => {
              try {
                for await (const event of events) {
                  if (controller.signal.aborted) {
                    break
                  }
                  onEvent(event)
                }
              } catch (error) {
                if (!controller.signal.aborted) {
                  throw error
                }
              }
            })()
            return () => controller.abort()
          },
        },
      },
    }
  },
})
