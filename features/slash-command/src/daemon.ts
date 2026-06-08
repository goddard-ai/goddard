import { definePlugin } from "@goddard-ai/daemon-plugin"

import { slashCommandIpcRoutes } from "./daemon-ipc.ts"
import { listSlashCommands, resolveSlashCommand } from "./daemon/resolver.ts"
import { mergeSlashCommandsConfigLayers, SlashCommandsConfig } from "./schema.ts"

export const slashCommandPlugin = definePlugin({
  name: "slash-command",
  config: {
    slashCommands: {
      schema: SlashCommandsConfig,
      scopes: ["user", "project"],
      resolve: ({ project, user }) =>
        mergeSlashCommandsConfigLayers({
          project,
          user,
        }),
    },
  },
  ipcRoutes: slashCommandIpcRoutes,
  setup({ configProvider }) {
    return {
      ipcHandlers: {
        slashCommand: {
          list: async ({ body }) => listSlashCommands(body, configProvider),
          resolve: async ({ body }) => resolveSlashCommand(body, configProvider),
        },
      },
    }
  },
})
