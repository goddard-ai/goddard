import { useEffect } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { AppCommand } from "~/commands/app-command.ts"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import styles from "./launch-form-selectors.style.ts"
import { flattenConfigOptionValues, type SessionLaunchFormState } from "./launch-form-state.ts"

function getShortcutLabel(binding: unknown) {
  if (typeof binding === "string") {
    return binding
  }

  if (binding && typeof binding === "object") {
    if ("combo" in binding && typeof binding.combo === "string") {
      return binding.combo
    }

    if ("sequence" in binding && typeof binding.sequence === "string") {
      return binding.sequence
    }
  }

  return null
}

export component SessionLaunchLoadingSelect(props: { label: string }) {
  <SessionInputSelect
    disabled={true}
    filterable={false}
    items={[]}
    label={props.label}
    loading={true}
    open={false}
    placeholder="Loading options..."
    value={null}
    onOpenChange={() => {}}
    onValueChange={() => {}}
  />
}

export component AgentHarnessSelector(props: { form: SessionLaunchFormState }) {
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const adapterCatalog = useQuery(goddardSdk.adapter.list, [
    { cwd: form.draftProjectPath.value ?? undefined },
  ])
  const items: SessionInputSelectItem[] = adapterCatalog.adapters.map(
    (adapter) => ({
      value: adapter.id,
      label: adapter.name,
      detail: `${adapter.version}${adapter.unofficial ? " · Unofficial" : ""}`,
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
    open={form.openPicker.value === "adapter"}
    placeholder="Select an adapter"
    shortcut={adapterShortcut}
    value={form.draftAdapterId.value}
    onOpenChange={(open) => {
      form.setOpenPicker(open ? "adapter" : null)
    }}
    onValueChange={(value) => {
      form.draftAdapterId.value = value
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
    form.draftProjectPath.value && form.draftAdapterId.value
      ? goddardSdk.session.launchPreview
      : null,
    form.draftProjectPath.value && form.draftAdapterId.value
      ? [
          {
            agent: form.draftAdapterId.value,
            cwd: form.draftProjectPath.value,
          },
        ]
      : null,
  )

  useEffect(() => {
    form.launchPreview.value = preview
  }, [form, preview])

  const locationItems: SessionInputSelectItem[] = [
    {
      value: "local",
      label: "Local checkout",
    },
    ...(preview?.repoRoot
      ? [
          {
            value: "worktree",
            label: "Linked worktree",
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
  const modelShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openModelSelector.id]?.[0],
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
      open={form.openPicker.value === "location"}
      placeholder="Choose where to launch"
      shortcut={locationShortcut}
      value={form.draftLocation.value}
      onOpenChange={(open) => {
        form.setOpenPicker(open ? "location" : null)
      }}
      onValueChange={(value) => {
        form.draftLocation.value = value as "local" | "worktree"
      }}
    />
    <SessionInputSelect
      disabled={form.draftLocation.value !== "worktree" || branchItems.length === 0}
      filterable={true}
      items={branchItems}
      label="Git branch"
      open={form.openPicker.value === "branch"}
      placeholder="Choose a branch"
      shortcut={branchShortcut}
      value={form.draftBaseBranchName.value}
      onOpenChange={(open) => {
        form.setOpenPicker(open ? "branch" : null)
      }}
      onValueChange={(value) => {
        form.draftBaseBranchName.value = value
      }}
    />
  </div>
  <div class={styles.grid}>
    <SessionInputSelect
      disabled={modelItems.length === 0}
      filterable={true}
      items={modelItems}
      label="Model"
      open={form.openPicker.value === "model"}
      placeholder="Use the adapter default model"
      shortcut={modelShortcut}
      value={form.draftModelId.value}
      onOpenChange={(open) => {
        form.setOpenPicker(open ? "model" : null)
      }}
      onValueChange={(value) => {
        form.draftModelId.value = value
      }}
    />
    <SessionInputSelect
      disabled={thinkingItems.length === 0}
      filterable={false}
      items={thinkingItems}
      label="Thinking level"
      open={form.openPicker.value === "thinking"}
      placeholder="Use the adapter default"
      shortcut={thinkingShortcut}
      value={typeof form.draftThinkingValue.value === "boolean"
        ? String(form.draftThinkingValue.value)
        : form.draftThinkingValue.value ?? null}
      onOpenChange={(open) => {
        form.setOpenPicker(open ? "thinking" : null)
      }}
      onValueChange={(value) => {
        form.draftThinkingValue.value = form.thinkingOption.value?.type === "boolean"
          ? value === "true"
          : value
      }}
    />
  </div>
}
