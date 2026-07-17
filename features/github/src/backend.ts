import { defineBackendPlugin } from "@goddard-ai/backend-plugin"

import { githubBackendProviders } from "./backend/providers.ts"
import { githubBackendRoutes } from "./backend/routes.ts"

export const githubBackendPlugin = defineBackendPlugin({
  name: "github",
  routes: githubBackendRoutes,
  providers: githubBackendProviders,
})

export * from "./backend/app.ts"
export * from "./backend/events.ts"
export * from "./backend/providers.ts"
export * from "./backend/routes.ts"
