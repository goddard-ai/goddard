import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { authIpcRoutes } from "./daemon-ipc.ts"
import type { DeviceFlowComplete, DeviceFlowStart } from "./schema.ts"

export const authSdkPlugin = defineSdkPlugin({
  name: "auth",
  ipcRoutes: authIpcRoutes,
  wrap({ client }) {
    return {
      auth: {
        /** Starts one GitHub device flow through the daemon auth contract. */
        startDeviceFlow: (input?: DeviceFlowStart) => client.auth.device.start(input),

        /** Completes one pending GitHub device flow through the daemon auth contract. */
        completeDeviceFlow: (input: DeviceFlowComplete) => client.auth.device.complete(input),
      },
    }
  },
})
