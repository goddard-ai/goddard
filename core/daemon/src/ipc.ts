import { initializeDaemonPluginComposition } from "./ipc/server.ts"

initializeDaemonPluginComposition()

export { startDaemonServer } from "./ipc/server.ts"
export { createDaemonUrl, readDaemonTcpAddressFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
export type { DaemonServer } from "./ipc/types.ts"
