import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { slashCommandIpcRoutes } from "./daemon-ipc.ts"

export const slashCommandSdkPlugin = defineSdkPlugin({
  name: "slash-command",
  ipcRoutes: slashCommandIpcRoutes,
})
