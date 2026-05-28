import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { reviewSessionIpcRoutes } from "./daemon-ipc.ts"

export const reviewSessionSdkPlugin = defineSdkPlugin({
  name: "review-session",
  ipcRoutes: reviewSessionIpcRoutes,
})
