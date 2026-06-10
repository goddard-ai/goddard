/** ACP launch-preview model normalization helpers shared by SDK consumers. */
import type {
  DaemonSessionModelState,
  InitialSessionConfigOption,
} from "@goddard-ai/session/schema"
import * as acp from "acp-client/protocol"

const derivedThinkingConfigId = "_goddard_derived_thinking_level"
const thinkingLevelOrder = ["none", "minimal", "low", "medium", "high", "xhigh", "max"] as const
const thinkingLevelLabels = new Map(
  thinkingLevelOrder.map((value) => [value, value[0].toUpperCase() + value.slice(1)]),
)

type SelectSessionConfigOption = Extract<acp.SessionConfigOption, { type: "select" }>

function isConfigOptionGroup(option: unknown): option is acp.SessionConfigSelectGroup {
  return (
    typeof option === "object" &&
    option !== null &&
    "options" in option &&
    Array.isArray(option.options)
  )
}

function flattenConfigOptionValues(option: SelectSessionConfigOption) {
  return option.options.flatMap((entry) => (isConfigOptionGroup(entry) ? entry.options : [entry]))
}

function findModelConfigOption(configOptions: acp.SessionConfigOption[]) {
  return (
    configOptions.find(
      (option): option is SelectSessionConfigOption =>
        option.category === "model" && option.type === "select",
    ) ?? null
  )
}

function parseThinkingModelName(name: string) {
  const match = /^(?<baseName>.+?)\s+\((?<thinking>[^()]+)\)$/.exec(name.trim())
  if (!match?.groups) {
    return null
  }

  const normalizedThinking = match.groups.thinking
    .trim()
    .replace(/[\s_-]+/g, "")
    .toLowerCase()

  if (!thinkingLevelOrder.includes(normalizedThinking as (typeof thinkingLevelOrder)[number])) {
    return null
  }

  return {
    baseName: match.groups.baseName.trim(),
    thinkingValue: normalizedThinking,
  }
}

function parseThinkingModelId(modelId: string) {
  const match = /^(?<baseName>.+?)\/(?<thinking>[^/]+)$/.exec(modelId.trim())
  if (!match?.groups) {
    return null
  }

  const normalizedThinking = match.groups.thinking
    .trim()
    .replace(/[\s_-]+/g, "")
    .toLowerCase()

  if (!thinkingLevelOrder.includes(normalizedThinking as (typeof thinkingLevelOrder)[number])) {
    return null
  }

  return {
    baseName: match.groups.baseName.trim(),
    thinkingValue: normalizedThinking,
  }
}

type SessionModelInfo = DaemonSessionModelState["availableModels"][number]

function parseThinkingModel(model: SessionModelInfo) {
  const parsedName = parseThinkingModelName(model.name)
  if (parsedName) {
    return parsedName
  }

  const parsedId = parseThinkingModelId(model.modelId)
  if (!parsedId) {
    return null
  }

  return {
    ...parsedId,
    baseName: model.name.trim() || parsedId.baseName,
  }
}

function sortThinkingOptions(values: Iterable<string>) {
  return [...new Set(values)].sort((left, right) => {
    const leftIndex = thinkingLevelOrder.indexOf(left as (typeof thinkingLevelOrder)[number])
    const rightIndex = thinkingLevelOrder.indexOf(right as (typeof thinkingLevelOrder)[number])

    if (leftIndex === -1 || rightIndex === -1) {
      return left.localeCompare(right)
    }

    return leftIndex - rightIndex
  })
}

function slugifyModelName(name: string) {
  return (
    name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "model"
  )
}

function createPassthroughLaunchModelConfig(input: {
  models: DaemonSessionModelState | null
  configOptions: acp.SessionConfigOption[]
}) {
  return {
    models: resolveSessionModelState(input.models),
    configOptions: input.configOptions,
    resolveSelection(input: {
      modelId?: string | null
      configOptions?: InitialSessionConfigOption[] | null
    }) {
      return {
        initialModelId: input.modelId ?? undefined,
        initialConfigOptions:
          (input.configOptions?.length ?? 0) > 0 ? [...input.configOptions!] : undefined,
      }
    },
  }
}

function resolveSessionModelState(models: DaemonSessionModelState | null) {
  if (!models || models.availableModels.length === 0) {
    return models
  }

  if (models.availableModels.some((model) => model.modelId === models.currentModelId)) {
    return models
  }

  return {
    ...models,
    currentModelId: models.availableModels[0]!.modelId,
  }
}

function createConfigOptionLaunchModelConfig(input: { configOptions: acp.SessionConfigOption[] }) {
  const modelOption = findModelConfigOption(input.configOptions)
  if (!modelOption) {
    return null
  }

  const availableModels = flattenConfigOptionValues(modelOption).map((option) => ({
    modelId: option.value,
    name: option.name,
    description: option.description,
  }))
  if (availableModels.length === 0) {
    return null
  }

  const currentModelId = availableModels.some((model) => model.modelId === modelOption.currentValue)
    ? modelOption.currentValue
    : availableModels[0]!.modelId

  return {
    models: {
      currentModelId,
      availableModels,
    },
    configOptions: input.configOptions,
    resolveSelection(input: {
      modelId?: string | null
      configOptions?: InitialSessionConfigOption[] | null
    }) {
      const remainingConfigOptions = (input.configOptions ?? []).filter(
        (option) => option.configId !== modelOption.id,
      )
      const selectedModelId = input.modelId ?? undefined

      return {
        initialModelId: undefined,
        initialConfigOptions: selectedModelId
          ? [
              ...remainingConfigOptions,
              {
                configId: modelOption.id,
                value: selectedModelId,
              },
            ]
          : remainingConfigOptions.length > 0
            ? remainingConfigOptions
            : undefined,
      }
    },
  }
}

/** Derives launch-time model and thinking selectors from one ACP launch preview. */
export function deriveSessionLaunchModelConfig(input: {
  models: DaemonSessionModelState | null
  configOptions: acp.SessionConfigOption[]
}) {
  const configOptionModelConfig = createConfigOptionLaunchModelConfig(input)
  if (configOptionModelConfig) {
    return configOptionModelConfig
  }

  const models = resolveSessionModelState(input.models)

  if (!models || models.availableModels.length === 0) {
    return createPassthroughLaunchModelConfig({ ...input, models })
  }

  const existingThinkingOption = input.configOptions.find(
    (option) => option.category === "thought_level",
  )

  if (existingThinkingOption && existingThinkingOption.type !== "select") {
    return createPassthroughLaunchModelConfig({ ...input, models })
  }

  const parsedModels = models.availableModels.map((model) => {
    const parsedModel = parseThinkingModel(model)
    return parsedModel ? { model, ...parsedModel } : null
  })

  if (parsedModels.some((model) => model === null)) {
    return createPassthroughLaunchModelConfig({ ...input, models })
  }

  const groups = new Map<
    string,
    {
      syntheticModelId: string
      variants: NonNullable<(typeof parsedModels)[number]>[]
    }
  >()

  for (const parsedModel of parsedModels) {
    if (!parsedModel) {
      continue
    }

    const existingGroup = groups.get(parsedModel.baseName)
    if (existingGroup) {
      existingGroup.variants.push(parsedModel)
      continue
    }

    groups.set(parsedModel.baseName, {
      syntheticModelId: `__goddard_model_${groups.size}_${slugifyModelName(parsedModel.baseName)}`,
      variants: [parsedModel],
    })
  }

  const thinkingOptions = sortThinkingOptions(
    parsedModels.flatMap((parsedModel) => (parsedModel ? [parsedModel.thinkingValue] : [])),
  )

  if (groups.size === models.availableModels.length || thinkingOptions.length < 2) {
    return createPassthroughLaunchModelConfig({ ...input, models })
  }

  const availableModels = [...groups.values()].map((group) => ({
    modelId: group.syntheticModelId,
    name: group.variants[0].baseName,
    description: group.variants.find((variant) => variant.model.description)?.model.description,
  }))
  const currentVariant = parsedModels.find(
    (parsedModel) => parsedModel?.model.modelId === models.currentModelId,
  )
  const currentGroup = currentVariant ? groups.get(currentVariant.baseName) : null

  return {
    models: {
      ...models,
      currentModelId: currentGroup?.syntheticModelId ?? models.currentModelId,
      availableModels,
    },
    configOptions: existingThinkingOption
      ? input.configOptions
      : [
          ...input.configOptions,
          {
            id: derivedThinkingConfigId,
            type: "select" as const,
            name: "Thinking level",
            category: "thought_level",
            description: "Derived from ACP model names.",
            currentValue: currentVariant?.thinkingValue ?? thinkingOptions[0],
            options: thinkingOptions.map((thinkingValue) => ({
              value: thinkingValue,
              name:
                thinkingLevelLabels.get(thinkingValue as (typeof thinkingLevelOrder)[number]) ??
                thinkingValue,
            })),
          } satisfies acp.SessionConfigOption,
        ],
    resolveSelection(input: {
      modelId?: string | null
      configOptions?: InitialSessionConfigOption[] | null
    }) {
      const thinkingConfigId = existingThinkingOption?.id ?? derivedThinkingConfigId
      const remainingConfigOptions = (input.configOptions ?? []).filter(
        (option) => option.configId !== derivedThinkingConfigId,
      )
      const selectedThinkingValue = input.configOptions?.find(
        (option) => option.configId === thinkingConfigId && "value" in option,
      )
      const selectedGroup = [...groups.values()].find(
        (group) => group.syntheticModelId === input.modelId,
      )

      if (!selectedGroup) {
        return {
          initialModelId: input.modelId ?? undefined,
          initialConfigOptions:
            remainingConfigOptions.length > 0 ? remainingConfigOptions : undefined,
        }
      }

      const matchingVariant =
        typeof selectedThinkingValue?.value === "string"
          ? selectedGroup.variants.find(
              (variant) => variant.thinkingValue === selectedThinkingValue.value,
            )
          : null
      const resolvedVariant = matchingVariant ?? selectedGroup.variants[0]

      return {
        initialModelId: resolvedVariant.model.modelId,
        initialConfigOptions:
          remainingConfigOptions.length > 0 ? remainingConfigOptions : undefined,
      }
    },
  }
}
