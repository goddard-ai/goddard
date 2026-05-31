import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { actionIpcRoutes } from "./daemon-ipc.ts"
import { buildNamedActionSessionParams, resolveNamedAction } from "./daemon/resolver.ts"
import { ActionConfig, mergeActionConfigLayers } from "./schema.ts"

export const actionPlugin = definePlugin({
  name: "action",
  consumes: [sessionPlugin],
  config: {
    actions: {
      schema: ActionConfig,
      scopes: ["user", "project"],
      resolve: ({ project, user }) => mergeActionConfigLayers(user, project),
    },
  },
  jsonSchemas: [{ name: "action.json", schema: ActionConfig }],
  ipcRoutes: actionIpcRoutes,
  setup({ configProvider, getIpcRequestContext, session }) {
    return {
      ipcHandlers: {
        action: {
          run: async ({ body: payload }) => {
            const action = await resolveNamedAction(payload.actionName, payload.cwd, configProvider)
            const response = {
              session: await session.newSession({
                request: buildNamedActionSessionParams(action, payload.cwd, {
                  cwd: payload.cwd,
                  agent: payload.agent,
                  mcpServers: payload.mcpServers,
                  env: payload.env,
                  systemPrompt: payload.systemPrompt,
                  repository: payload.repository,
                  prNumber: payload.prNumber,
                  metadata: payload.metadata,
                }),
              }),
            }
            getIpcRequestContext().setSessionId(response.session.id)
            return response
          },
        },
      },
    }
  },
})
