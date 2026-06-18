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

import { lens } from "~/lib/lens.ts"
import { isEmptyQuery } from "~/lib/search-query.ts"
import { hasPromptContent } from "~/session-chat/composer-content.ts"
import {
  findSessionModeConfigOption,
  flattenConfigOptionValues,
} from "~/session-input/config-options.ts"
import { preferredLaunchAgentId, resolvePreferredLaunchAgentId } from "./launch-preferences.ts"

type ComposerPromptBlocks = Exclude<SessionPromptRequest["prompt"], string>
type SessionLaunchPickerId =
  | "project"
  | "subpackage"
  | "adapter"
  | "branch"
  | "model"
  | "mode"
  | "thinking"
type LaunchPickerId = SessionLaunchPickerId | null
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
  const draftModeValue = signal<string | null>(null)
  const draftProjectPath = signal<string | null>(null)
  const draftPromptBlocks = signal<ComposerPromptBlocks>([])
  const draftSubpackagePath = signal<string | null>(null)
  const draftThinkingValue = signal<string | boolean | null>(null)
  const launchWorktreeId = signal<string | null>(null)
  const launchPreview = signal<SessionLaunchPreviewResponse | null>(null)
  const openPicker = signal<LaunchPickerId>(null)

  const launchModelConfig = computed(() =>
    deriveSessionLaunchModelConfig({
      configOptions: launchPreview.value?.configOptions ?? [],
    }),
  )
  const thinkingOption = computed(
    () =>
      launchModelConfig.value.configOptions.find((option) => option.category === "thought_level") ??
      null,
  )
  const modeOption = computed(() =>
    findSessionModeConfigOption(launchModelConfig.value.configOptions),
  )
  const effectiveCwd = computed(() => draftSubpackagePath.value ?? draftProjectPath.value)

  const sessionInput = computed<CreateSessionRequest | null>(() => {
    const agent = draftAdapterId.value
    const cwd = effectiveCwd.value
    const initialPrompt = draftPromptBlocks.value

    if (
      !agent ||
      !cwd ||
      !hasPromptContent(initialPrompt) ||
      (launchPreview.value?.bare === true && draftLocation.value === "local")
    ) {
      return null
    }

    const initialConfigOptions: InitialSessionConfigOption[] = []
    const resolvedModeOption = modeOption.value
    const resolvedThinkingOption = thinkingOption.value

    if (resolvedModeOption && typeof draftModeValue.value === "string") {
      initialConfigOptions.push({
        configId: resolvedModeOption.id,
        value: draftModeValue.value,
      })
    }

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
    const currentBranchName = launchPreview.value?.currentBranch ?? null
    const selectedLocalBranchName =
      draftLocation.value === "local" &&
      launchPreview.value?.dirty !== true &&
      draftBaseBranchName.value &&
      draftBaseBranchName.value !== currentBranchName
        ? draftBaseBranchName.value
        : null

    return {
      agent,
      cwd,
      launchLeaseId:
        draftLocation.value === "local" && !selectedLocalBranchName
          ? launchPreview.value?.launchLeaseId
          : undefined,
      localCheckout: selectedLocalBranchName ? { branchName: selectedLocalBranchName } : undefined,
      worktree:
        draftLocation.value === "worktree"
          ? {
              enabled: true,
              baseBranchName: draftBaseBranchName.value ?? undefined,
            }
          : undefined,
      launchWorktreeId:
        draftLocation.value === "worktree" ? (launchWorktreeId.value ?? undefined) : undefined,
      mcpServers: [],
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

    const nextAdapterId = resolvePreferredLaunchAgentId(nextAdapterCatalog)

    if (draftAdapterId.value !== nextAdapterId) {
      draftAdapterId.value = nextAdapterId
    }
  }

  function syncLaunchPreview(nextLaunchPreview: SessionLaunchPreviewResponse | null) {
    if (!nextLaunchPreview) {
      draftBaseBranchName.value = null
      draftModelId.value = null
      draftModeValue.value = null
      draftThinkingValue.value = null
      draftLocation.value = "local"
      return
    }

    if (nextLaunchPreview.bare) {
      draftLocation.value = "worktree"
    } else if (!nextLaunchPreview.repoRoot && draftLocation.value === "worktree") {
      draftLocation.value = "local"
    }

    const resolvedLaunchModelConfig = deriveSessionLaunchModelConfig({
      configOptions: nextLaunchPreview.configOptions,
    })
    const availableBranchNames = new Set(nextLaunchPreview.branches)
    const currentBranchName =
      nextLaunchPreview.currentBranch ?? nextLaunchPreview.branches[0] ?? null

    if (
      draftBaseBranchName.value === null ||
      !availableBranchNames.has(draftBaseBranchName.value) ||
      (nextLaunchPreview.dirty && draftLocation.value === "local")
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

    const resolvedModeOption = findSessionModeConfigOption(resolvedLaunchModelConfig.configOptions)

    if (!resolvedModeOption) {
      draftModeValue.value = null
    } else {
      const availableModeValues = new Set(
        flattenConfigOptionValues(resolvedModeOption).map((option) => option.value),
      )

      if (draftModeValue.value === null || !availableModeValues.has(draftModeValue.value)) {
        draftModeValue.value = resolvedModeOption.currentValue
      }
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

  function setLaunchLocation(nextLocation: SessionLaunchLocation) {
    const resolvedLocation =
      nextLocation === "local" && launchPreview.value?.bare
        ? "worktree"
        : nextLocation === "worktree" && !launchPreview.value?.repoRoot
          ? "local"
          : nextLocation

    draftLocation.value = resolvedLocation

    if (resolvedLocation === "local" && launchPreview.value?.dirty) {
      draftBaseBranchName.value = launchPreview.value.currentBranch ?? null
    }
  }

  function cycleLaunchLocation() {
    if (!launchPreview.value?.repoRoot) {
      setLaunchLocation("local")
      return
    }

    if (launchPreview.value.bare) {
      setLaunchLocation("worktree")
      return
    }

    setLaunchLocation(draftLocation.value === "local" ? "worktree" : "local")
  }

  adapterCatalog.subscribe(syncAdapterSelection)
  preferredLaunchAgentId.subscribe(() => {
    syncAdapterSelection(adapterCatalog.value)
  })
  launchPreview.subscribe(syncLaunchPreview)

  return {
    adapterCatalog,
    canSubmit: computed(() => sessionInput.value !== null),
    cycleLaunchLocation,
    draftAdapterId,
    draftBaseBranchName,
    draftLocation,
    draftModelId,
    draftModeValue,
    draftProjectPath,
    draftPromptBlocks,
    draftSubpackagePath,
    draftThinkingValue,
    effectiveCwd,
    launchModelConfig,
    launchPreview,
    launchWorktreeId,
    modeOption,
    openPicker,
    reset(preferredProjectPath: string | null = null) {
      const previousProjectPath = draftProjectPath.value
      draftAdapterId.value = null
      draftBaseBranchName.value = null
      draftLocation.value = "local"
      draftModelId.value = null
      draftModeValue.value = null
      draftProjectPath.value = preferredProjectPath
      draftPromptBlocks.value = []
      draftSubpackagePath.value = null
      draftThinkingValue.value = null
      launchWorktreeId.value = null
      launchPreview.value = null
      openPicker.value = null

      if (preferredProjectPath === previousProjectPath) {
        syncAdapterSelection(adapterCatalog.value)
      }
    },
    sessionInput,
    setLaunchLocation,
    getPickerOpen(picker: SessionLaunchPickerId) {
      return lens(
        () => openPicker.value === picker,
        (open) => {
          openPicker.value = open ? picker : openPicker.value === picker ? null : openPicker.value
        },
      )
    },
    setOpenPicker(nextPicker: LaunchPickerId) {
      openPicker.value = nextPicker
    },
    thinkingOption,
  }
})

export type SessionLaunchFormState = InstanceType<typeof SessionLaunchFormState>
