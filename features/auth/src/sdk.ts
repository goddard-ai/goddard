import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { authIpcRoutes } from "./daemon-ipc.ts"
import type { DeviceFlowComplete, DeviceFlowStart } from "./schema.ts"

export const authSdkPlugin = defineSdkPlugin({
  name: "auth",
  ipcRoutes: authIpcRoutes,
  extend({ client }) {
    return {
      auth: {
        /** Starts one GitHub device flow through the daemon auth contract. */
        startDeviceFlow: (input?: DeviceFlowStart) => client.send("auth.device.start", input),

        /** Completes one pending GitHub device flow through the daemon auth contract. */
        completeDeviceFlow: (input: DeviceFlowComplete) =>
          client.send("auth.device.complete", input),

        /** Reads the current daemon-owned auth session as-is. */
        whoami: () => client.send("auth.whoami"),

        /** Clears the current daemon-owned auth session as-is. */
        logout: () => client.send("auth.logout"),
      },
    }
  },
})
