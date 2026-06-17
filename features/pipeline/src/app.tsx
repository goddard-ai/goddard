import { pipelineSdkPlugin } from "./sdk.ts"

export type PipelineAppSdkRequirements = {
  readonly pipeline: ReturnType<NonNullable<typeof pipelineSdkPlugin.wrap>>["pipeline"]
}

export const pipelineAppPlugin = {
  name: "pipeline",
  sdk: {} as PipelineAppSdkRequirements,
  navigation: {
    slot: "primaryWorkbench",
    id: "pipelines",
    label: "Pipelines",
    icon: "tabs/tasks",
  },
  workbenchTab: {
    kind: "pipelines",
    icon: "tabs/tasks",
  },
  runWorkbenchTab: {
    kind: "pipelineRun",
    icon: "tabs/tasks",
  },
  commands: {
    openNavigation: {
      id: "navigation.openPipelines",
      label: "Open Pipelines",
      targetNavId: "pipelines",
    },
  },
} as const
