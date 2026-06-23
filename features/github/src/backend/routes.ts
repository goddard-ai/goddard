import { $type, defineBackendRoutes, http } from "@goddard-ai/backend-plugin"

import type { GitHubRemoteRepoEvent } from "./events.ts"

export const githubBackendRoutes = defineBackendRoutes({
  webhooks: http.resource("webhooks", {
    github: http.post("github", {
      body: http.rawBody(),
      response: $type<GitHubRemoteRepoEvent | { ignored: true }>(),
    }),
  }),
})
