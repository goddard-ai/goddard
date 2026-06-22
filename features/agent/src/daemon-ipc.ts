import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  InstallManagedAgentRequest,
  ListManagedAgentsRequest,
  UninstallManagedAgentRequest,
  type InstallManagedAgentResponse,
  type ListManagedAgentsResponse,
  type UninstallManagedAgentResponse,
} from "./schema.ts"

export const managedAgentIpcRoutes = defineIpcRoutes({
  managedAgent: http.resource("managed-agent", {
    /** Lists managed agents available for one project or global launch flow. */
    list: http.post("list", {
      body: ListManagedAgentsRequest,
      response: $type<ListManagedAgentsResponse>(),
    }),
    /** Installs one managed agent into the local launch catalog. */
    install: http.post("install", {
      body: InstallManagedAgentRequest,
      response: $type<InstallManagedAgentResponse>(),
    }),
    /** Removes one managed agent from the local launch catalog. */
    uninstall: http.post("uninstall", {
      body: UninstallManagedAgentRequest,
      response: $type<UninstallManagedAgentResponse>(),
    }),
  }),
})
