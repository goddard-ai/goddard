import type * as acp from "acp-client/protocol"

import type { DaemonSessionModelState } from "../schema.ts"

type SelectSessionConfigOption = Extract<acp.SessionConfigOption, { type: "select" }>

function isSelectConfigOption(
  option: acp.SessionConfigOption,
): option is SelectSessionConfigOption {
  return option.type === "select"
}

function flattenSelectOptions(options: acp.SessionConfigSelectOptions) {
  return options.flatMap((option) => ("group" in option ? option.options : [option]))
}

/** Projects ACP's config-option model selector into Goddard's persisted UI model state. */
export function deriveSessionModelState(
  configOptions: acp.SessionConfigOption[],
): DaemonSessionModelState | null {
  const modelOption = configOptions.find(
    (option): option is SelectSessionConfigOption =>
      option.category === "model" && isSelectConfigOption(option),
  )
  if (!modelOption) {
    return null
  }

  return {
    currentModelId: modelOption.currentValue,
    availableModels: flattenSelectOptions(modelOption.options).map((model) => ({
      modelId: model.value,
      name: model.name,
      description: model.description,
    })),
  }
}
