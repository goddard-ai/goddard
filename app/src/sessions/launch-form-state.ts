import {
  deriveSessionLaunchModelConfig,
  type CreateSessionRequest,
  type InitialSessionConfigOption,
  type ListAdaptersResponse,
  type SessionLaunchPreviewResponse,
  type SessionPromptRequest,
} from "@goddard-ai/sdk"
import { computed, createModel, signal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"

import { isEmptyQuery } from "~/lib/search-query.ts"
import { hasPromptContent } from "~/session-chat/composer-content.ts"

type ComposerPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
type LaunchPickerId = "project" | "adapter" | "location" | "branch" | "model" | "thinking" | null
/** One slash-command suggestion shown in the session launch composer. */
type SlashCommandSuggestion = SessionLaunchPreviewResponse["slashCommands"][number]
/** One prepared slash-command suggestion cached by source array identity. */
type PreparedSlashCommandSuggestion = {
  suggestion: SlashCommandSuggestion
  preparedDescription: fuzzysort.PreparedTarget | null
  preparedInputHint: fuzzysort.PreparedTarget | null
  preparedName: fuzzysort.PreparedTarget
}

export type SessionLaunchLocation = "local" | "worktree"

const preparedSlashCommandSuggestions = new WeakMap<
  readonly SlashCommandSuggestion[],
  readonly PreparedSlashCommandSuggestion[]
>()

function isConfigOptionGroup(option: unknown): option is {
  name: string
  options: Array<{ value: string; name: string; description?: string | null }>
} {
  return (
    typeof option === "object" &&
    option !== null &&
    "options" in option &&
    Array.isArray(option.options)
  )
}

/** Returns the prepared slash-command suggestions cached for one suggestion array instance. */
function getPreparedSlashCommandSuggestions(suggestions: readonly SlashCommandSuggestion[]) {
  const existingSuggestions = preparedSlashCommandSuggestions.get(suggestions)

  if (existingSuggestions) {
    return existingSuggestions
  }

  const nextSuggestions = suggestions.map((suggestion) => ({
    preparedDescription: suggestion.description ? fuzzysort.prepare(suggestion.description) : null,
    preparedInputHint: suggestion.inputHint ? fuzzysort.prepare(suggestion.inputHint) : null,
    preparedName: fuzzysort.prepare(suggestion.name),
    suggestion,
  }))

  preparedSlashCommandSuggestions.set(suggestions, nextSuggestions)
  return nextSuggestions
}

/** Flattens grouped select options into one ordered list of concrete values. */
export function flattenConfigOptionValues(
  option: Extract<
    NonNullable<SessionLaunchPreviewResponse["configOptions"]>[number],
    { type: "select" }
  >,
) {
  const flattenedOptions: Array<{
    value: string
    name: string
    description?: string | null
  }> = []

  for (const entry of option.options) {
    if (isConfigOptionGroup(entry)) {
      flattenedOptions.push(...entry.options)
      continue
    }

    flattenedOptions.push(entry)
  }

  return flattenedOptions
}

/** Fuzzy-filters slash-command suggestions while preserving the default result cap. */
export function filterSlashCommandSuggestions(
  suggestions: readonly SlashCommandSuggestion[],
  query: string,
  limit = 20,
) {
  if (isEmptyQuery(query)) {
    return suggestions.slice(0, limit)
  }

  return fuzzysort
    .searchFields(
      query,
      getPreparedSlashCommandSuggestions(suggestions),
      [
        { key: "name", extract: (entry) => entry.preparedName },
        { key: "description", extract: (entry) => entry.preparedDescription },
        { key: "inputHint", extract: (entry) => entry.preparedInputHint },
      ],
      { limit, threshold: 0 },
    )
    .items.map((entry) => entry.value.suggestion)
}

export const SessionLaunchFormState = createModel(function () {
  const adapterCatalog = signal<ListAdaptersResponse | null>(null)
  const draftAdapterId = signal<string | null>(null)
  const draftBaseBranchName = signal<string | null>(null)
  const draftLocation = signal<SessionLaunchLocation>("local")
  const draftModelId = signal<string | null>(null)
  const draftProjectPath = signal<string | null>(null)
  const draftPromptBlocks = signal<ComposerPromptBlocks>([])
  const draftThinkingValue = signal<string | boolean | null>(null)
  const launchPreview = signal<SessionLaunchPreviewResponse | null>(null)
  const openPicker = signal<LaunchPickerId>(null)

  const selectedAdapter = computed(
    () =>
      adapterCatalog.value?.adapters.find((adapter) => adapter.id === draftAdapterId.value) ?? null,
  )
  const launchModelConfig = computed(() =>
    deriveSessionLaunchModelConfig({
      models: launchPreview.value?.models ?? null,
      configOptions: launchPreview.value?.configOptions ?? [],
    }),
  )
  const thinkingOption = computed(
    () =>
      launchModelConfig.value.configOptions.find((option) => option.category === "thought_level") ??
      null,
  )

  const sessionInput = computed<CreateSessionRequest | null>(() => {
    const agent = draftAdapterId.value
    const cwd = draftProjectPath.value
    const initialPrompt = draftPromptBlocks.value

    if (!agent || !cwd || !hasPromptContent(initialPrompt)) {
      return null
    }

    const initialConfigOptions: InitialSessionConfigOption[] = []
    const resolvedThinkingOption = thinkingOption.value

    if (
      resolvedThinkingOption?.type === "boolean" &&
      typeof draftThinkingValue.value === "boolean"
    ) {
      initialConfigOptions.push({
        configId: resolvedThinkingOption.id,
        type: "boolean",
        value: draftThinkingValue.value,
      })
    }

    if (resolvedThinkingOption?.type === "select" && typeof draftThinkingValue.value === "string") {
      initialConfigOptions.push({
        configId: resolvedThinkingOption.id,
        value: draftThinkingValue.value,
      })
    }

    const resolvedSelection = launchModelConfig.value.resolveSelection({
      modelId: draftModelId.value,
      configOptions: initialConfigOptions,
    })

    return {
      agent,
      cwd,
      worktree:
        draftLocation.value === "worktree"
          ? {
              enabled: true,
              baseBranchName: draftBaseBranchName.value ?? undefined,
            }
          : undefined,
      mcpServers: [],
      systemPrompt: "",
      initialModelId: resolvedSelection.initialModelId,
      initialConfigOptions: resolvedSelection.initialConfigOptions,
      initialPrompt,
    }
  })

  function syncAdapterSelection(nextAdapterCatalog: ListAdaptersResponse | null) {
    if (!nextAdapterCatalog) {
      draftAdapterId.value = null
      return
    }

    const availableAdapterIds = new Set(nextAdapterCatalog.adapters.map((adapter) => adapter.id))
    const nextAdapterId =
      draftAdapterId.value && availableAdapterIds.has(draftAdapterId.value)
        ? draftAdapterId.value
        : nextAdapterCatalog.defaultAdapterId &&
            availableAdapterIds.has(nextAdapterCatalog.defaultAdapterId)
          ? nextAdapterCatalog.defaultAdapterId
          : (nextAdapterCatalog.adapters[0]?.id ?? null)

    if (draftAdapterId.value !== nextAdapterId) {
      draftAdapterId.value = nextAdapterId
    }
  }

  function syncLaunchPreview(nextLaunchPreview: SessionLaunchPreviewResponse | null) {
    if (!nextLaunchPreview) {
      draftBaseBranchName.value = null
      draftModelId.value = null
      draftThinkingValue.value = null
      draftLocation.value = "local"
      return
    }

    if (!nextLaunchPreview.repoRoot && draftLocation.value === "worktree") {
      draftLocation.value = "local"
    }

    const resolvedLaunchModelConfig = deriveSessionLaunchModelConfig({
      models: nextLaunchPreview.models ?? null,
      configOptions: nextLaunchPreview.configOptions,
    })
    const availableBranchNames = new Set(nextLaunchPreview.branches.map((branch) => branch.name))
    const currentBranchName =
      nextLaunchPreview.branches.find((branch) => branch.current)?.name ??
      nextLaunchPreview.branches[0]?.name ??
      null

    if (
      draftBaseBranchName.value === null ||
      !availableBranchNames.has(draftBaseBranchName.value)
    ) {
      draftBaseBranchName.value = currentBranchName
    }

    const availableModelIds = new Set(
      resolvedLaunchModelConfig.models?.availableModels.map((model) => model.modelId) ?? [],
    )
    const currentModelId = resolvedLaunchModelConfig.models?.currentModelId ?? null

    if (
      draftModelId.value === null ||
      (draftModelId.value && !availableModelIds.has(draftModelId.value))
    ) {
      draftModelId.value = currentModelId
    }

    const resolvedThinkingOption =
      resolvedLaunchModelConfig.configOptions.find(
        (option) => option.category === "thought_level",
      ) ?? null

    if (!resolvedThinkingOption) {
      draftThinkingValue.value = null
      return
    }

    if (resolvedThinkingOption.type === "boolean") {
      if (typeof draftThinkingValue.value !== "boolean") {
        draftThinkingValue.value = resolvedThinkingOption.currentValue
      }

      return
    }

    const availableThinkingValues = new Set(
      flattenConfigOptionValues(resolvedThinkingOption).map((option) => option.value),
    )

    if (
      typeof draftThinkingValue.value !== "string" ||
      !availableThinkingValues.has(draftThinkingValue.value)
    ) {
      draftThinkingValue.value = resolvedThinkingOption.currentValue
    }
  }

  adapterCatalog.subscribe(syncAdapterSelection)
  launchPreview.subscribe(syncLaunchPreview)

  return {
    adapterCatalog,
    canSubmit: computed(() => sessionInput.value !== null),
    draftAdapterId,
    draftBaseBranchName,
    draftLocation,
    draftModelId,
    draftProjectPath,
    draftPromptBlocks,
    draftThinkingValue,
    launchModelConfig,
    launchPreview,
    openPicker,
    reset(preferredProjectPath: string | null = null) {
      const previousProjectPath = draftProjectPath.value
      draftAdapterId.value = null
      draftBaseBranchName.value = null
      draftLocation.value = "local"
      draftModelId.value = null
      draftProjectPath.value = preferredProjectPath
      draftPromptBlocks.value = []
      draftThinkingValue.value = null
      launchPreview.value = null
      openPicker.value = null

      if (preferredProjectPath === previousProjectPath) {
        syncAdapterSelection(adapterCatalog.value)
      }
    },
    selectedAdapter,
    sessionInput,
    setOpenPicker(nextPicker: LaunchPickerId) {
      openPicker.value = nextPicker
    },
    thinkingOption,
  }
})

export type SessionLaunchFormState = InstanceType<typeof SessionLaunchFormState>
