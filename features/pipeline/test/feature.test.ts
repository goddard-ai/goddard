import { describe, expect, test } from "bun:test"

import { pipelineAppPlugin } from "../src/app.tsx"
import { pipelineIpcRoutes } from "../src/daemon-ipc.ts"
import { pipelinePlugin } from "../src/daemon.ts"
import { pipelineSdkPlugin } from "../src/sdk.ts"

describe("pipeline feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(pipelineAppPlugin.name).toBe("pipeline")
    expect(pipelineAppPlugin.navigation).toMatchObject({ id: "pipelines" })
    expect(pipelinePlugin.name).toBe("pipeline")
    expect(Object.keys(pipelineIpcRoutes.pipeline.children)).toEqual([
      "listDefinitions",
      "listDefinitionDiagnostics",
      "spawnRun",
      "getRun",
      "listRuns",
      "cancelRun",
      "advanceRun",
      "approveRun",
      "retryRun",
    ])
    expect(pipelineSdkPlugin.name).toBe("pipeline")
  })
})
