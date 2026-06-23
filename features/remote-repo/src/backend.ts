import { defineBackendPlugin } from "@goddard-ai/backend-plugin"

import { remoteRepoBackendEvents, remoteRepoBackendEventSources } from "./backend/events.ts"
import { remoteRepoBackendRoutes } from "./backend/routes.ts"

export const remoteRepoBackendPlugin = defineBackendPlugin({
  name: "remote-repo",
  routes: remoteRepoBackendRoutes,
  events: remoteRepoBackendEvents,
  eventSources: remoteRepoBackendEventSources,
})

export * from "./backend/events.ts"
export * from "./backend/routes.ts"
export * from "./backend/stream.ts"
