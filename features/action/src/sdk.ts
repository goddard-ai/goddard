import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { actionIpcRoutes } from "./daemon-ipc.ts"
import type { RunNamedActionRequest } from "./schema.ts"

export const actionSdkPlugin = defineSdkPlugin({
  name: "action",
  ipcRoutes: actionIpcRoutes,
  wrap({ client }) {
    return {
      action: {
        /** Runs one named daemon action and creates the resulting daemon session. */
        run: (input: RunNamedActionRequest) => client.action.run({ body: input }),
      },
    }
  },
})
