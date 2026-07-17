import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import { sessionEvents } from "./events.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
  events: sessionEvents,
})
