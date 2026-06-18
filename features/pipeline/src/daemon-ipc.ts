import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  AdvancePipelineRunRequest,
  ApprovePipelineRunRequest,
  CancelPipelineRunRequest,
  GetPipelineRunRequest,
  ListPipelineDefinitionsRequest,
  ListPipelineRunsRequest,
  RetryPipelineRunRequest,
  SpawnPipelineRunRequest,
  type AdvancePipelineRunResponse,
  type ApprovePipelineRunResponse,
  type CancelPipelineRunResponse,
  type GetPipelineRunResponse,
  type ListPipelineDefinitionDiagnosticsResponse,
  type ListPipelineDefinitionsResponse,
  type ListPipelineRunsResponse,
  type RetryPipelineRunResponse,
  type SpawnPipelineRunResponse,
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
    /** Creates a queued pipeline run and its ordered step-run records. */
    spawnRun: http.post("runs/spawn", {
      body: SpawnPipelineRunRequest,
      response: $type<SpawnPipelineRunResponse>(),
    }),
    /** Reads one pipeline run with ordered step-run records. */
    getRun: http.post("runs/get", {
      body: GetPipelineRunRequest,
      response: $type<GetPipelineRunResponse>(),
    }),
    /** Lists persisted pipeline runs using daemon ordering and filtering. */
    listRuns: http.post("runs/list", {
      body: ListPipelineRunsRequest,
      response: $type<ListPipelineRunsResponse>(),
    }),
    /** Cancels a queued, waiting, or running pipeline run at metadata level. */
    cancelRun: http.post("runs/cancel", {
      body: CancelPipelineRunRequest,
      response: $type<CancelPipelineRunResponse>(),
    }),
    /** Advances a run until it succeeds, fails, waits, or needs agent execution. */
    advanceRun: http.post("runs/advance", {
      body: AdvancePipelineRunRequest,
      response: $type<AdvancePipelineRunResponse>(),
    }),
    /** Approves the current waiting approval step and continues the run. */
    approveRun: http.post("runs/approve", {
      body: ApprovePipelineRunRequest,
      response: $type<ApprovePipelineRunResponse>(),
    }),
    /** Resets a failed script step and later queued steps, then advances the run. */
    retryRun: http.post("runs/retry", {
      body: RetryPipelineRunRequest,
      response: $type<RetryPipelineRunResponse>(),
    }),
  }),
})
