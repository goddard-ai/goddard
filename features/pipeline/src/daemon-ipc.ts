import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  ListPipelineDefinitionsRequest,
  type ListPipelineDefinitionDiagnosticsResponse,
  type ListPipelineDefinitionsResponse,
} from "./schema.ts"

export const pipelineIpcRoutes = defineIpcRoutes({
  pipeline: http.resource("pipeline", {
    /** Lists registered pipeline definitions for one project root. */
    listDefinitions: http.post("definitions/list", {
      body: ListPipelineDefinitionsRequest,
      response: $type<ListPipelineDefinitionsResponse>(),
    }),
    /** Lists pipeline definition loading and registration diagnostics. */
    listDefinitionDiagnostics: http.post("definitions/diagnostics", {
      body: ListPipelineDefinitionsRequest,
      response: $type<ListPipelineDefinitionDiagnosticsResponse>(),
    }),
  }),
})
