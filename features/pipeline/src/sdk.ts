import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { pipelineIpcRoutes } from "./daemon-ipc.ts"

export const pipelineSdkPlugin = defineSdkPlugin({
  name: "pipeline",
  ipcRoutes: pipelineIpcRoutes,
})
