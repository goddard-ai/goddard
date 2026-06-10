import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  InstallAdapterRequest,
  ListAdaptersRequest,
  UninstallAdapterRequest,
  type InstallAdapterResponse,
  type ListAdaptersResponse,
  type UninstallAdapterResponse,
} from "./schema.ts"

export const adapterIpcRoutes = defineIpcRoutes({
  adapter: http.resource("adapter", {
    /** Lists adapters available for one project or global launch flow. */
    list: http.post("list", {
      body: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    }),
    /** Installs one adapter into the local launch catalog. */
    install: http.post("install", {
      body: InstallAdapterRequest,
      response: $type<InstallAdapterResponse>(),
    }),
    /** Removes one adapter from the local launch catalog. */
    uninstall: http.post("uninstall", {
      body: UninstallAdapterRequest,
      response: $type<UninstallAdapterResponse>(),
    }),
  }),
})
