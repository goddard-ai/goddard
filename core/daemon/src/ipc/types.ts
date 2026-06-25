import type { BackendEventHandler, EventBus, EventDefinition } from "@goddard-ai/daemon-plugin"

import type { daemonRuntimeEvents } from "../events.ts"

export type DaemonServerEvents = EventBus<
  typeof daemonRuntimeEvents,
  Record<string, EventDefinition<unknown>>
>

export type DaemonServer = {
  backendEventHandlers: readonly BackendEventHandler<any>[]
  daemonUrl: string
  events: DaemonServerEvents
  port: number
  close: () => Promise<void>
}
