/** ACP launch-preview model normalization helpers shared by SDK consumers. */
import * as acp from "@agentclientprotocol/sdk"
import type { InitialSessionConfigOption } from "@goddard-ai/schema/daemon"

const derivedThinkingConfigId = "_goddard_derived_thinking_level"
const thinkingLevelOrder = ["none", "minimal", "low", "medium", "high", "xhigh", "max"] as const
const thinkingLevelLabels = new Map(
  thinkingLevelOrder.map((value) => [value, value[0].toUpperCase() + value.slice(1)]),
)

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
  models: acp.SessionModelState | null
  configOptions: acp.SessionConfigOption[]
}) {
  return {
    models: input.models,
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

/** Derives launch-time model and thinking selectors from one ACP launch preview. */
export function deriveSessionLaunchModelConfig(input: {
  models: acp.SessionModelState | null
  configOptions: acp.SessionConfigOption[]
}) {
  if (
    !input.models ||
    input.configOptions.some((option) => option.category === "thought_level") ||
    input.models.availableModels.length === 0
  ) {
    return createPassthroughLaunchModelConfig(input)
  }

  const parsedModels = input.models.availableModels.map((model) => {
    const parsedName = parseThinkingModelName(model.name)
    return parsedName ? { model, ...parsedName } : null
  })

  if (parsedModels.some((model) => model === null)) {
    return createPassthroughLaunchModelConfig(input)
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

  if (groups.size === input.models.availableModels.length || thinkingOptions.length < 2) {
    return createPassthroughLaunchModelConfig(input)
  }

  const availableModels = [...groups.values()].map((group) => ({
    modelId: group.syntheticModelId,
    name: group.variants[0].baseName,
    description: group.variants.find((variant) => variant.model.description)?.model.description,
  }))
  const currentVariant = parsedModels.find(
    (parsedModel) => parsedModel?.model.modelId === input.models?.currentModelId,
  )
  const currentGroup = currentVariant ? groups.get(currentVariant.baseName) : null

  return {
    models: {
      ...input.models,
      currentModelId: currentGroup?.syntheticModelId ?? input.models.currentModelId,
      availableModels,
    },
    configOptions: [
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
      const remainingConfigOptions = (input.configOptions ?? []).filter(
        (option) => option.configId !== derivedThinkingConfigId,
      )
      const selectedThinkingValue = input.configOptions?.find(
        (option) => option.configId === derivedThinkingConfigId && "value" in option,
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
