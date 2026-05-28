import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { actionIpcRoutes } from "./daemon-ipc.ts"
import { buildNamedActionSessionParams, resolveNamedAction } from "./daemon/resolver.ts"
import { ActionConfig } from "./schema.ts"

export const actionPlugin = definePlugin({
  name: "action",
  consumes: [sessionPlugin],
  config: {
    schema: ActionConfig,
    scopes: ["user", "project"],
  },
  ipcRoutes: actionIpcRoutes,
  setup({ configManager, getIpcRequestContext, session }) {
    return {
      ipcHandlers: {
        action: {
          run: async ({ body: payload }) => {
            const action = await resolveNamedAction(payload.actionName, payload.cwd, configManager)
            const response = {
              session: await session.create(
                buildNamedActionSessionParams(action, payload.cwd, {
                  cwd: payload.cwd,
                  agent: payload.agent,
                  mcpServers: payload.mcpServers,
                  env: payload.env,
                  systemPrompt: payload.systemPrompt,
                  repository: payload.repository,
                  prNumber: payload.prNumber,
                  metadata: payload.metadata,
                }),
              ),
            }
            getIpcRequestContext().setSessionId(response.session.id)
            return response
          },
        },
      },
    }
  },
})
