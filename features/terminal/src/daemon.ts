import { definePlugin } from "@goddard-ai/daemon-plugin"

import { terminalIpcRoutes } from "./daemon-ipc.ts"

export { runTerminalRuntimeCheck, type TerminalRuntimeCheckResult } from "./daemon/self-test.ts"
export { DaemonTerminalError, DaemonTerminalManager } from "./daemon/runtime.ts"
export type { DaemonTerminalManagerOptions } from "./daemon/runtime.ts"

export const terminalPlugin = definePlugin({
  name: "terminal",
  ipcRoutes: terminalIpcRoutes,
})
