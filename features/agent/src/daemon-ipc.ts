import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  InstallAgentRequest,
  ListAgentsRequest,
  UninstallAgentRequest,
  type InstallAgentResponse,
  type ListAgentsResponse,
  type UninstallAgentResponse,
} from "./schema.ts"

export const agentIpcRoutes = defineIpcRoutes({
  agent: http.resource("agent", {
    /** Lists agents available for one project or global launch flow. */
    list: http.post("list", {
      body: ListAgentsRequest,
      response: $type<ListAgentsResponse>(),
    }),
    /** Installs one agent into the local launch catalog. */
    install: http.post("install", {
      body: InstallAgentRequest,
      response: $type<InstallAgentResponse>(),
    }),
    /** Removes one agent from the local launch catalog. */
    uninstall: http.post("uninstall", {
      body: UninstallAgentRequest,
      response: $type<UninstallAgentResponse>(),
    }),
  }),
})
