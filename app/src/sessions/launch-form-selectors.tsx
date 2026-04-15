import { css } from "@goddard-ai/styled-system/css"
import { Bot, Command, Folder, GitBranch } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsx"
import { flattenConfigOptionValues, type SessionLaunchFormState } from "./launch-form-state.ts"

export const sessionLaunchSectionClass = css({
  display: "grid",
  gap: "14px",
})

export const sessionLaunchSelectorGridClass = css({
  display: "grid",
  gap: "12px",
  "@media (min-width: 720px)": {
    gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
  },
})

export const sessionLaunchAdapterCardClass = css({
  display: "grid",
  gap: "4px",
  padding: "12px",
  borderRadius: "14px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
})

export function SessionLaunchLoadingSelect(props: { label: string }) {
  return (
    <SessionInputSelect
      disabled={true}
      filterable={false}
      items={[]}
      label={props.label}
      loading={true}
      menuLabel={props.label}
      open={false}
      placeholder="Loading options..."
      value={null}
      onOpenChange={() => {}}
      onValueChange={() => {}}
    />
  )
}

export function AgentHarnessSelector(props: { form: SessionLaunchFormState }) {
  const { form } = props
  const adapterCatalog = useQuery(goddardSdk.adapter.list, [
    { cwd: form.draftProjectPath.value ?? undefined },
  ])
  const items: SessionInputSelectItem[] = adapterCatalog.adapters.map((adapter) => ({
    value: adapter.id,
    label: adapter.name,
    detail: `${adapter.version}${adapter.unofficial ? " · Unofficial" : ""}`,
    searchText: `${adapter.id}\n${adapter.description ?? ""}`,
    icon: Command,
  }))

  useEffect(() => {
    form.adapterCatalog.value = adapterCatalog
  }, [adapterCatalog, form])

  return (
    <SessionInputSelect
      disabled={!form.draftProjectPath.value}
      filterable={true}
      items={items}
      label="Adapter"
      menuLabel="Adapters"
      open={form.openPicker.value === "adapter"}
      placeholder="Select an adapter"
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
  )
}

export function SessionLaunchPreviewSelectors(props: { form: SessionLaunchFormState }) {
  const { form } = props
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
      detail: "Start the session in the current project working tree.",
      icon: Folder,
    },
    ...(preview?.repoRoot
      ? [
          {
            value: "worktree",
            label: "Linked worktree",
            detail: "Create a fresh session branch from the selected git branch.",
            icon: GitBranch,
          } satisfies SessionInputSelectItem,
        ]
      : []),
  ]
  const branchItems: SessionInputSelectItem[] =
    preview?.branches.map((branch) => ({
      value: branch.name,
      label: branch.name,
      detail: branch.current ? "Current branch" : null,
      icon: GitBranch,
    })) ?? []
  const modelItems: SessionInputSelectItem[] =
    preview?.models?.availableModels.map((model) => ({
      value: model.modelId,
      label: model.name,
      detail: model.description ?? model.modelId,
      searchText: model.modelId,
      icon: Bot,
    })) ?? []
  const thinkingItems: SessionInputSelectItem[] =
    form.thinkingOption.value?.type === "boolean"
      ? [
          {
            value: "true",
            label: "On",
            detail: "Enable the adapter's thinking mode.",
            icon: Command,
          },
          {
            value: "false",
            label: "Off",
            detail: "Use the adapter's faster non-thinking mode.",
            icon: Command,
          },
        ]
      : form.thinkingOption.value?.type === "select"
        ? flattenConfigOptionValues(form.thinkingOption.value).map((option) => ({
            value: option.value,
            label: option.name,
            detail: option.description ?? null,
            icon: Command,
          }))
        : []

  return (
    <>
      <div class={sessionLaunchSelectorGridClass}>
        <SessionInputSelect
          disabled={!form.draftProjectPath.value}
          filterable={false}
          items={locationItems}
          label="Launch location"
          menuLabel="Launch location"
          open={form.openPicker.value === "location"}
          placeholder="Choose where to launch"
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
          menuLabel="Git branch"
          open={form.openPicker.value === "branch"}
          placeholder="Choose a branch"
          value={form.draftBaseBranchName.value}
          onOpenChange={(open) => {
            form.setOpenPicker(open ? "branch" : null)
          }}
          onValueChange={(value) => {
            form.draftBaseBranchName.value = value
          }}
        />
      </div>
      <div class={sessionLaunchSelectorGridClass}>
        <SessionInputSelect
          disabled={modelItems.length === 0}
          filterable={true}
          items={modelItems}
          label="Model"
          menuLabel="Models"
          open={form.openPicker.value === "model"}
          placeholder="Use the adapter default model"
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
          menuLabel="Thinking level"
          open={form.openPicker.value === "thinking"}
          placeholder="Use the adapter default"
          value={
            typeof form.draftThinkingValue.value === "boolean"
              ? String(form.draftThinkingValue.value)
              : (form.draftThinkingValue.value ?? null)
          }
          onOpenChange={(open) => {
            form.setOpenPicker(open ? "thinking" : null)
          }}
          onValueChange={(value) => {
            form.draftThinkingValue.value =
              form.thinkingOption.value?.type === "boolean" ? value === "true" : value
          }}
        />
      </div>
    </>
  )
}
