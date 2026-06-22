import type { BackendEventHandler, EventBus, EventDefinition } from "@goddard-ai/daemon-plugin"

export type DaemonServer = {
  backendEventHandlers: readonly BackendEventHandler<any>[]
  daemonUrl: string
  events: EventBus<Record<string, EventDefinition<unknown>>>
  port: number
  close: () => Promise<void>
}
