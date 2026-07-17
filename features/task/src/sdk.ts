import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { taskIpcRoutes } from "./daemon-ipc.ts"
import { taskEvents } from "./events.ts"

export const taskSdkPlugin = defineSdkPlugin({
  name: "task",
  ipcRoutes: taskIpcRoutes,
  events: taskEvents,
})
