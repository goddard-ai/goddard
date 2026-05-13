import { definePlugin, defineSetupContext } from "@goddard-ai/daemon-plugin"

import { sessionIpcSchema } from "./daemon-ipc.ts"
import { createSessionExtension, type SessionSetupContext } from "./daemon/extension.ts"
import { createSessionRequestHandlers } from "./daemon/handlers.ts"

export type { SessionController, SessionSetupContext } from "./daemon/extension.ts"

export const sessionPlugin = definePlugin({
  name: "session",
  ipc: sessionIpcSchema,
  setupContext: defineSetupContext<SessionSetupContext>(),
  setup(context) {
    const session = createSessionExtension(context.controller)

    return {
      provides: {
        session,
      },
      requestHandlers: createSessionRequestHandlers({
        session,
        setRequestSessionId: context.setRequestSessionId,
      }),
    }
  },
})
