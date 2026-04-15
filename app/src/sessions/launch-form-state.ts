import type {
  CreateSessionRequest,
  InitialSessionConfigOption,
  ListAdaptersResponse,
  SessionComposerSuggestionsResponse,
  SessionLaunchPreviewResponse,
  SessionPromptRequest,
} from "@goddard-ai/sdk"
import { computed, createModel, signal } from "@preact/signals"

import { hasPromptContent } from "~/session-chat/composer-content.ts"

type ComposerPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
type LaunchPickerId = "project" | "adapter" | "location" | "branch" | "model" | "thinking" | null

export type SessionLaunchLocation = "local" | "worktree"

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

export function filterSlashCommandSuggestions(
  suggestions: readonly SessionLaunchPreviewResponse["slashCommands"][number][],
  query: string,
  limit = 20,
) {
  const normalizedQuery = query.trim().toLowerCase()
  const filteredSuggestions: SessionComposerSuggestionsResponse["suggestions"] = []

  for (const suggestion of suggestions) {
    if (
      normalizedQuery.length > 0 &&
      suggestion.name.toLowerCase().includes(normalizedQuery) === false &&
      suggestion.description.toLowerCase().includes(normalizedQuery) === false &&
      (suggestion.inputHint?.toLowerCase().includes(normalizedQuery) ?? false) === false
    ) {
      continue
    }

    filteredSuggestions.push(suggestion)

    if (filteredSuggestions.length >= limit) {
      break
    }
  }

  return filteredSuggestions
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
  const thinkingOption = computed(
    () =>
      launchPreview.value?.configOptions.find((option) => option.category === "thought_level") ??
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
      initialModelId: draftModelId.value ?? undefined,
      initialConfigOptions: initialConfigOptions.length > 0 ? initialConfigOptions : undefined,
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
      nextLaunchPreview.models?.availableModels.map((model) => model.modelId) ?? [],
    )
    const currentModelId = nextLaunchPreview.models?.currentModelId ?? null

    if (
      draftModelId.value === null ||
      (draftModelId.value && !availableModelIds.has(draftModelId.value))
    ) {
      draftModelId.value = currentModelId
    }

    const resolvedThinkingOption =
      nextLaunchPreview.configOptions.find((option) => option.category === "thought_level") ?? null

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
    toggleThinkingLevel() {
      const resolvedThinkingOption = thinkingOption.value

      if (!resolvedThinkingOption) {
        return
      }

      if (resolvedThinkingOption.type === "boolean") {
        draftThinkingValue.value =
          typeof draftThinkingValue.value === "boolean"
            ? !draftThinkingValue.value
            : resolvedThinkingOption.currentValue
        return
      }

      const values = flattenConfigOptionValues(resolvedThinkingOption)

      if (values.length === 0) {
        return
      }

      const currentIndex = values.findIndex((option) => option.value === draftThinkingValue.value)
      const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % values.length : 0
      draftThinkingValue.value = values[nextIndex]?.value ?? resolvedThinkingOption.currentValue
    },
  }
})

export type SessionLaunchFormState = InstanceType<typeof SessionLaunchFormState>
