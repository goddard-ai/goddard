import type { SessionSubpackage } from "@goddard-ai/sdk"
import { useSignal } from "@preact/signals"
import { GitFork, Laptop } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { AppCommand } from "~/commands/app-command.ts"
import { getPreferredShortcutLabel, getShortcutLabel } from "~/commands/shortcut-label.ts"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { flattenConfigOptionValues } from "~/session-input/config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import { SessionModeSelector } from "~/session-input/session-mode-selector.tsrx"
import styles from "./launch-form-selectors.style.ts"
import { type SessionLaunchFormState } from "./launch-form-state.ts"
import { releaseSessionLaunchLease } from "./mutations.ts"

export component SessionLaunchLoadingSelect(props: { label: string }) {
  const open = useSignal(false)

  <SessionInputSelect
    disabled={true}
    filterable={false}
    items={[]}
    label={props.label}
    loading={true}
    {open}
    placeholder="Loading options..."
    value={null}
    onValueChange={() => {}}
  />
}

export component AgentHarnessSelector(props: { form: SessionLaunchFormState }) {
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const adapterCatalog = useQuery(goddardSdk.adapter.list, {
    params: [{ cwd: form.draftProjectPath.value ?? undefined }],
  })
  const items: SessionInputSelectItem[] = adapterCatalog.adapters.map(
    (adapter) => ({
      value: adapter.id,
      label: adapter.name,
      detail: `${adapter.version}${adapter.unofficial ? " · Unofficial" : ""}`,
      icon: adapter.icon,
      searchText: `${adapter.id}\n${adapter.description ?? ""}`,
    }),
  )
  const adapterShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openAdapterSelector.id]?.[0],
  )

  useEffect(() => {
    form.adapterCatalog.value = adapterCatalog
  }, [adapterCatalog, form])

  <SessionInputSelect
    disabled={!form.draftProjectPath.value}
    filterable={true}
    {items}
    label="Adapter"
    open={form.getPickerOpen("adapter")}
    placeholder="Select an adapter"
    shortcut={adapterShortcut}
    value={form.draftAdapterId.value}
    onValueChange={(value) => {
      form.draftAdapterId.value = value
      form.launchPreview.value = null
      form.setOpenPicker(null)
    }}
  />
}

/** Builds the explicit project-root option followed by discovered package boundaries. */
function buildSubpackageItems(projectPath: string, subpackages: readonly SessionSubpackage[]) {
  return [
    {
      value: projectPath,
      label: "Project root",
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

export component SessionSubpackageSelector(props: { form: SessionLaunchFormState }) {
  const { form } = props
  const projectPath = form.draftProjectPath.value
  const response = useQuery(projectPath ? goddardSdk.session.subpackages : null, {
    params: projectPath ? [{ cwd: projectPath }] : null,
  })
  const subpackages = projectPath ? response.subpackages : []
  const items =
    projectPath ? buildSubpackageItems(projectPath, subpackages) : []
  const selectedValue = form.draftSubpackagePath.value ?? projectPath

  useEffect(() => {
    if (!projectPath || form.draftSubpackagePath.value === null) {
      return
    }

    if (!subpackages.some((subpackage) => subpackage.path === form.draftSubpackagePath.value)) {
      form.draftSubpackagePath.value = null
      form.launchPreview.value = null
    }
  }, [form, projectPath, subpackages])

  if (subpackages.length === 0) {
    return
  }

  <SessionInputSelect
    disabled={!projectPath}
    filterable={true}
    {items}
    label="Working directory"
    open={form.getPickerOpen("subpackage")}
    placeholder="Project root"
    value={selectedValue}
    onValueChange={(value) => {
      form.draftSubpackagePath.value = value === projectPath ? null : value
      form.launchPreview.value = null
      form.setOpenPicker(null)
    }}
  />
}

export component SessionLaunchPreviewSelectors(props: {
  form: SessionLaunchFormState;
}) {
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const preview = useQuery(
    form.effectiveCwd.value && form.draftAdapterId.value ? goddardSdk.session.launchPreview : null,
    {
      params: form.effectiveCwd.value && form.draftAdapterId.value
        ? [
            {
              agent: form.draftAdapterId.value,
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
        if (!launchLeaseId || form.launchPreview.value?.launchLeaseId === launchLeaseId) {
          return
        }

        void releaseSessionLaunchLease(launchLeaseId).catch((error) => {
          console.error("Failed to release launch lease.", error)
        })
      })
    }
  }, [form, preview])

  const locationItems: SessionInputSelectItem[] = [
    {
      value: "local",
      label: "Work locally",
      Icon: Laptop,
    },
    ...(preview?.repoRoot
      ? [
          {
            value: "worktree",
            label: "New worktree",
            Icon: GitFork,
          },
        ]
      : []),
  ]
  const branchItems: SessionInputSelectItem[] =
    preview?.branches.map(
      (branch) => ({
        value: branch.name,
        label: branch.name,
        detail: branch.current ? "Current branch" : null,
      }),
    ) ??
      []
  const currentBranchName = preview?.branches.find((branch) => branch.current)?.name ?? null
  const localCheckoutDirty = form.draftLocation.value === "local" && preview?.dirty === true
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
            label: "On",
          },
          {
            value: "false",
            label: "Off",
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
  const locationShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openLocationSelector.id]?.[0],
  )
  const branchShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openBranchSelector.id]?.[0],
  )
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

  <div class={styles.grid}>
    <SessionInputSelect
      disabled={!form.draftProjectPath.value}
      filterable={false}
      items={locationItems}
      label="Launch location"
      open={form.getPickerOpen("location")}
      placeholder="Choose where to launch"
      shortcut={locationShortcut}
      value={form.draftLocation.value}
      onValueChange={(value) => {
        form.draftLocation.value = value as "local" | "worktree"

        if (value === "local" && preview?.dirty) {
          form.draftBaseBranchName.value = currentBranchName
        }
      }}
    />
    <SessionInputSelect
      disabled={branchItems.length === 0 || localCheckoutDirty}
      filterable={true}
      items={branchItems}
      label="Git branch"
      open={form.getPickerOpen("branch")}
      placeholder="Choose a branch"
      shortcut={branchShortcut}
      value={form.draftBaseBranchName.value}
      onValueChange={(value) => {
        form.draftBaseBranchName.value = value
      }}
    />
  </div>

  if (localCheckoutDirty) {
    <p class={styles.warning}>"Commit or stash local changes to launch from another branch."</p>
  }

  <div class={styles.grid}>
    if (form.modeOption.value) {
      <SessionModeSelector
        modeOption={form.modeOption.value}
        open={form.getPickerOpen("mode")}
        shortcut={approvalPresetShortcut}
        onValueChange={(value) => {
          form.draftModeValue.value = value
        }}
      />
    }

    <SessionInputSelect
      disabled={modelItems.length === 0}
      filterable={true}
      items={modelItems}
      label="Model"
      open={form.getPickerOpen("model")}
      placeholder="Use the adapter default model"
      shortcut={modelShortcut}
      value={form.draftModelId.value}
      onValueChange={(value) => {
        form.draftModelId.value = value
      }}
    />
  </div>

  if (form.thinkingOption.value) {
    <div class={styles.grid}>
      <SessionInputSelect
        disabled={thinkingItems.length === 0}
        filterable={false}
        items={thinkingItems}
        label="Thinking level"
        open={form.getPickerOpen("thinking")}
        placeholder="Use the adapter default"
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
    </div>
  }
}
