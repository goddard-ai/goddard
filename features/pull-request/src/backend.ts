import { defineBackendPlugin } from "@goddard-ai/backend-plugin"

import { pullRequestBackendRoutes } from "./backend/routes.ts"

export const pullRequestBackendPlugin = defineBackendPlugin({
  name: "pull-request",
  routes: pullRequestBackendRoutes,
})

export * from "./backend/routes.ts"
