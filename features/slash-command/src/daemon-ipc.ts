import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  ListSlashCommandsRequest,
  ResolveSlashCommandRequest,
  type ListSlashCommandsResponse,
  type ResolveSlashCommandResponse,
} from "./schema.ts"

export const slashCommandIpcRoutes = defineIpcRoutes({
  slashCommand: http.resource("slash-command", {
    /** Lists custom slash commands visible from one working directory. */
    list: http.post("list", {
      body: ListSlashCommandsRequest,
      response: $type<ListSlashCommandsResponse>(),
    }),
    /** Resolves one custom slash command into the prompt submitted to an agent. */
    resolve: http.post("resolve", {
      body: ResolveSlashCommandRequest,
      response: $type<ResolveSlashCommandResponse>(),
    }),
  }),
})
