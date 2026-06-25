import { event } from "@goddard-ai/daemon-plugin"

export type BackendStreamDegradedEvent = {
  reason: "stream_failed" | "unauthenticated"
  errorMessage: string
}

export type BackendStreamStartedEvent = {
  daemonUrl: string
  port: number
}

export type ConfigReloadFailedEvent = {
  watchScope: "global" | "local"
  cwd: string
  localConfigPath: string
  errorMessage: string
  version?: number
}

/** Daemon-owned events produced outside feature plugin setup. */
export const daemonRuntimeEvents = {
  "backend.stream.started": event<BackendStreamStartedEvent>(),
  "backend.stream.degraded": event<BackendStreamDegradedEvent>(),
  "config.reload.failed": event<ConfigReloadFailedEvent>(),
}
