import { t } from "@lingui/core/macro"
import type { SessionSubpackage } from "@goddard-ai/sdk"
import { useSignal } from "@preact/signals"
import { GitFork, Laptop } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { getPreferredShortcutLabel, getShortcutLabel } from "~/commands/shortcut-label.ts"
import { GoodKeyboardShortcut } from "~/lib/good-keyboard-shortcut.tsrx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { flattenConfigOptionValues, stepConfigOptionValue } from "~/session-input/config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import { SessionModeSelector } from "~/session-input/session-mode-selector.tsrx"
import styles from "./launch-form-selectors.style.ts"
import { type SessionLaunchFormState } from "./launch-form-state.ts"
import {
  recordSessionLaunchAgentUse,
  recordSessionLaunchCwdUse,
  resolvePreferredLaunchCwd,
} from "./launch-preferences.ts"
import { isKeptSessionLaunchLease } from "./launch-lease-keeper.tsrx"
import { releaseSessionLaunchLease } from "./mutations.ts"

export function SessionLaunchLoadingSelect(props: { label: string; compact?: boolean }) @{
  const open = useSignal(false)

  <SessionInputSelect
    compact={props.compact}
    disabled={true}
    filterable={false}
    items={[]}
    label={props.label}
    loading={true}
    {open}
    placeholder={t`Loading options...`}
    value={null}
    onValueChange={() => {}}
  />
}

export function SessionLaunchPreviewLoadingSelectors(props: { compact?: boolean }) @{
  <div class={styles.composerControls}>
    <SessionLaunchLoadingSelect compact={props.compact} label={t`Agent harness`} />
    <SessionLaunchLoadingSelect compact={props.compact} label={t`Model`} />
    <SessionLaunchLoadingSelect compact={props.compact} label={t`Thinking level`} />
    <div class={styles.composerApprovalControl}>
      <SessionLaunchLoadingSelect compact={props.compact} label={t`Approval preset`} />
    </div>
  </div>
}

export function AgentHarnessSelector(props: {
  disabled?: boolean;
  compact?: boolean;
  form: SessionLaunchFormState;
}) @{
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const managedAgentCatalog = useQuery(goddardSdk.agent.list, {
    params: [{ cwd: form.effectiveCwd.value ?? undefined }],
  })
  const items: SessionInputSelectItem[] = managedAgentCatalog.agents.map(
    (adapter) => ({
      value: adapter.id,
      label: adapter.name,
      detail: `${adapter.version}${adapter.unofficial ? ` · ${t`Unofficial`}` : ""}`,
      icon: adapter.icon,
      searchText: `${adapter.id}\n${adapter.description ?? ""}`,
    }),
  )
  const adapterShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openAdapterSelector.id]?.[0],
  )

  useEffect(() => {
    form.managedAgentCatalog.value = managedAgentCatalog
  }, [managedAgentCatalog, form])

  <SessionInputSelect
    compact={props.compact}
    disabled={props.disabled || !form.draftProjectPath.value}
    filterable={true}
    iconOnlyTrigger={props.compact}
    {items}
    label={t`Agent harness`}
    open={form.getPickerOpen("agent")}
    placeholder={t`Select agent harness`}
    shortcut={adapterShortcut}
    value={form.draftManagedAgentId.value}
    onValueChange={(value) => {
      form.draftManagedAgentId.value = value
      recordSessionLaunchAgentUse(value)
      form.launchPreview.value = null
      form.setOpenPicker(null)
    }}
  />
}

export function SessionLaunchBranchSelector(props: {
  disabled?: boolean;
  form: SessionLaunchFormState;
}) @{
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const preview = useQuery(
    form.effectiveCwd.value && form.draftManagedAgentId.value
      ? goddardSdk.session.launchPreview
      : null,
    {
      params: form.effectiveCwd.value && form.draftManagedAgentId.value
        ? [
            {
              agent: form.draftManagedAgentId.value,
              cwd: form.effectiveCwd.value,
            },
          ]
        : null,
    },
  )
  const branchItems: SessionInputSelectItem[] =
    preview?.branches.map(
      (branch) => ({
        value: branch,
        label: branch,
        detail: branch === preview.currentBranch ? t`Current branch` : null,
      }),
    ) ??
      []
  const localCheckoutUnavailable =
    form.draftLocation.value === "local" && (preview?.bare === true || preview?.dirty === true)
  const branchShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openBranchSelector.id]?.[0],
  )

  <div class={styles.branchSelector}>
    <SessionInputSelect
      disabled={props.disabled || branchItems.length === 0 || localCheckoutUnavailable}
      filterable={true}
      items={branchItems}
      label={t`Git branch`}
      open={form.getPickerOpen("branch")}
      placeholder={t`Choose a branch`}
      shortcut={branchShortcut}
      value={form.draftBaseBranchName.value}
      onValueChange={(value) => {
        form.draftBaseBranchName.value = value
      }}
    />

    @if (form.draftLocation.value === "local" && preview?.dirty === true) {
      <p class={styles.warning}>
        {t`Commit or stash local changes to launch from another branch.`}
      </p>
    }
  </div>
}

export function SessionLaunchLocationSelector(props: {
  disabled?: boolean;
  form: SessionLaunchFormState;
}) @{
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const preview = useQuery(
    form.effectiveCwd.value && form.draftManagedAgentId.value
      ? goddardSdk.session.launchPreview
      : null,
    {
      params: form.effectiveCwd.value && form.draftManagedAgentId.value
        ? [
            {
              agent: form.draftManagedAgentId.value,
              cwd: form.effectiveCwd.value,
            },
          ]
        : null,
    },
  )

  useEffect(() => {
    form.launchPreview.value = preview

    return () => {
      const launchLeaseId = preview?.launchLeaseId

      if (form.launchPreview.value?.launchLeaseId === launchLeaseId) {
        form.launchPreview.value = null
      }

      queueMicrotask(() => {
        if (
          !launchLeaseId || form.launchPreview.value?.launchLeaseId === launchLeaseId ||
            isKeptSessionLaunchLease(launchLeaseId)
        ) {
          return
        }

        void releaseSessionLaunchLease(launchLeaseId).catch((error) => {
          console.error("Failed to release launch lease.", error)
        })
      })
    }
  }, [form, preview])

  if (!preview?.repoRoot) {
    return null
  }

  const locationOptions = [
    {
      value: "local",
      label: t`Work locally`,
      icon: Laptop,
    },
    {
      value: "worktree",
      label: t`New worktree`,
      icon: GitFork,
    },
  ] as const
  const locationShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openLocationSelector.id]?.[0],
  )

  <fieldset class={styles.radioField} disabled={props.disabled || !form.draftProjectPath.value}>
    <legend class={styles.radioLegend}>
      <span>
        {t`Launch location`}
      </span>

      @if (locationShortcut) {
        <GoodKeyboardShortcut expression={locationShortcut} />
      }
    </legend>
    <div class={styles.radioGroup}>
      @for (const option of locationOptions) {
        const OptionIcon = option.icon

        <label
          class={styles.radioOption}
          data-selected={form.draftLocation.value === option.value ? "true" : undefined}
          key={option.value}
        >
          <span>
            <OptionIcon
              aria-hidden={true}
              class={styles.radioOptionIcon}
              size={16}
              strokeWidth={2.1}
            />
            {option.label}
          </span>
          <input
            checked={form.draftLocation.value === option.value}
            disabled={option.value === "local" && preview.bare}
            name="session-launch-location"
            type="radio"
            value={option.value}
            onInput={() => {
              form.setLaunchLocation(option.value)
            }}
          />
        </label>
      }
    </div>
  </fieldset>
}

/** Builds the explicit project-root option followed by discovered package boundaries. */
function buildSubpackageItems(projectPath: string, subpackages: readonly SessionSubpackage[]) {
  return [
    {
      value: projectPath,
      label: t`Project root`,
      detail: projectPath,
      searchText: projectPath,
    },
    ...subpackages.map(
      (subpackage) => ({
        value: subpackage.path,
        label: subpackage.relativePath,
        detail: subpackage.manifestPath,
        searchText: subpackage.relativePath.split(/[\\/]/).at(-1) ?? subpackage.relativePath,
      }),
    ),
  ]
}

export function SessionSubpackageSelector(props: {
  disabled?: boolean;
  form: SessionLaunchFormState;
}) @{
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const projectPath = form.draftProjectPath.value
  const response = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })
  const subpackages = projectPath ? response.subpackages : []
  const items =
    projectPath ? buildSubpackageItems(projectPath, subpackages) : []
  const preferredCwd =
    projectPath ? resolvePreferredLaunchCwd(projectPath, subpackages) : null
  const selectedValue = preferredCwd ?? projectPath
  const subpackageShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openSubpackageSelector.id]?.[0],
  )

  useEffect(() => {
    if (!projectPath) {
      return
    }

    const nextSubpackagePath =
      preferredCwd === projectPath ? null : preferredCwd

    if (form.draftSubpackagePath.value !== nextSubpackagePath) {
      form.draftSubpackagePath.value = nextSubpackagePath
      form.launchPreview.value = null
    }
  }, [form, preferredCwd, projectPath])

  if (subpackages.length === 0) {
    return null
  }

  <SessionInputSelect
    disabled={props.disabled || !projectPath}
    filterable={true}
    {items}
    label={t`Working directory`}
    open={form.getPickerOpen("subpackage")}
    placeholder={t`Project root`}
    shortcut={subpackageShortcut}
    value={selectedValue}
    onValueChange={(value) => {
      form.draftSubpackagePath.value = value === projectPath ? null : value
      recordSessionLaunchCwdUse(projectPath, value)
      form.launchPreview.value = null
      form.setOpenPicker(null)
    }}
  />
}

export function SessionLaunchPreviewSelectors(props: {
  compact?: boolean;
  disabled?: boolean;
  form: SessionLaunchFormState;
}) @{
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  useQuery(
    form.effectiveCwd.value && form.draftManagedAgentId.value
      ? goddardSdk.session.launchPreview
      : null,
    {
      params: form.effectiveCwd.value && form.draftManagedAgentId.value
        ? [
            {
              agent: form.draftManagedAgentId.value,
              cwd: form.effectiveCwd.value,
            },
          ]
        : null,
    },
  )

  const previewModels = form.launchModelConfig.value.models
  const modelItems: SessionInputSelectItem[] =
    previewModels?.availableModels.map(
      (model) => ({
        value: model.modelId,
        label: model.name,
        detail: model.description ?? model.modelId,
        searchText: [model.name, model.description, model.modelId].filter(Boolean).join("\n"),
      }),
    ) ??
      []
  const thinkingItems: SessionInputSelectItem[] =
    form.thinkingOption.value?.type === "boolean"
      ? [
          {
            value: "true",
            label: t`On`,
          },
          {
            value: "false",
            label: t`Off`,
          },
        ]
      : form.thinkingOption.value?.type === "select"
        ? flattenConfigOptionValues(form.thinkingOption.value).map(
            (option) => ({
              value: option.value,
              label: option.name,
              detail: option.description ?? null,
            }),
          )
        : []
  const approvalPresetShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openApprovalPresetSelector.id]?.[0],
  )
  const modelShortcut = getPreferredShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openModelSelector.id],
    "Mod+/",
  )
  const thinkingShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openThinkingLevelSelector.id]?.[0],
  )

  useAppCommand(AppCommand.sessionInput.decreaseThinkingLevel, () => {
    const thinkingOption = form.thinkingOption.value
    if (thinkingOption?.type !== "select") {
      return
    }

    form.draftThinkingValue.value = stepConfigOptionValue(
      thinkingOption,
      typeof form.draftThinkingValue.value === "string" ? form.draftThinkingValue.value : null,
      -1,
    )
  }, { active: !props.disabled && thinkingItems.length > 0 })
  useAppCommand(AppCommand.sessionInput.increaseThinkingLevel, () => {
    const thinkingOption = form.thinkingOption.value
    if (thinkingOption?.type !== "select") {
      return
    }

    form.draftThinkingValue.value = stepConfigOptionValue(
      thinkingOption,
      typeof form.draftThinkingValue.value === "string" ? form.draftThinkingValue.value : null,
      1,
    )
  }, { active: !props.disabled && thinkingItems.length > 0 })

  <div class={props.compact ? styles.composerControls : styles.grid}>
    <AgentHarnessSelector compact={props.compact} disabled={props.disabled} {form} />

    <SessionInputSelect
      compact={props.compact}
      disabled={props.disabled || modelItems.length === 0}
      filterable={true}
      items={modelItems}
      label={t`Model`}
      open={form.getPickerOpen("model")}
      placeholder={t`Use the agent harness default model`}
      shortcut={modelShortcut}
      value={form.draftModelId.value}
      onValueChange={(value) => {
        form.draftModelId.value = value
      }}
    />

    @if (form.thinkingOption.value) {
      <SessionInputSelect
        compact={props.compact}
        disabled={props.disabled || thinkingItems.length === 0}
        filterable={false}
        items={thinkingItems}
        label={t`Thinking level`}
        open={form.getPickerOpen("thinking")}
        placeholder={t`Use the agent harness default`}
        shortcut={thinkingShortcut}
        value={typeof form.draftThinkingValue.value === "boolean"
          ? String(form.draftThinkingValue.value)
          : form.draftThinkingValue.value ?? null}
        onValueChange={(value) => {
          form.draftThinkingValue.value = form.thinkingOption.value?.type === "boolean"
            ? value === "true"
            : value
        }}
      />
    }

    @if (form.modeOption.value) {
      <div class={styles.composerApprovalControl}>
        <SessionModeSelector
          compact={props.compact}
          disabled={props.disabled}
          modeOption={form.modeOption.value}
          open={form.getPickerOpen("mode")}
          shortcut={approvalPresetShortcut}
          onValueChange={(value) => {
            form.draftModeValue.value = value
          }}
        />
      </div>
    }
  </div>
}
