import { event } from "@goddard-ai/daemon-plugin"

export type BackendStreamDegradedEvent = {
  reason: "unauthenticated"
  errorMessage: string
}

/** Daemon-owned events produced outside feature plugin setup. */
export const daemonRuntimeEvents = {
  "backend.stream.degraded": event<BackendStreamDegradedEvent>(),
}
