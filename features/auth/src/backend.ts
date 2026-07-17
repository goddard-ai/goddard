import { defineBackendPlugin } from "@goddard-ai/backend-plugin"

import { authBackendRoutes } from "./backend/routes.ts"

export const authBackendPlugin = defineBackendPlugin({
  name: "auth",
  routes: authBackendRoutes,
})

export * from "./backend/routes.ts"
