import type { BackendEventHandler } from "@goddard-ai/daemon-plugin"

import type { DaemonRuntimeEventBus } from "../events.ts"

export type DaemonServer = {
  backendEventHandlers: readonly BackendEventHandler<any>[]
  daemonUrl: string
  events: DaemonRuntimeEventBus
  port: number
  close: () => Promise<void>
}
