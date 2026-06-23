import { defineBackendPlugin } from "@goddard-ai/backend-plugin"

import { githubBackendRoutes } from "./backend/routes.ts"

export const githubBackendPlugin = defineBackendPlugin({
  name: "github",
  routes: githubBackendRoutes,
})

export * from "./backend/app.ts"
export * from "./backend/events.ts"
export * from "./backend/routes.ts"
