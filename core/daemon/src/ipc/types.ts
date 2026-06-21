import type { EventBus, EventDefinition } from "@goddard-ai/daemon-plugin"

export type DaemonServer = {
  daemonUrl: string
  events: EventBus<Record<string, EventDefinition<unknown>>>
  port: number
  close: () => Promise<void>
}
