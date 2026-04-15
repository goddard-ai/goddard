import { css } from "@goddard-ai/styled-system/css"
import { Folder } from "lucide-react"
import { Suspense } from "preact/compat"

import type { ProjectRecord } from "~/projects/project-registry.ts"
import { goddardSdk } from "~/sdk.ts"
import { Composer } from "~/session-chat/composer.tsx"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsx"
import {
  sessionLaunchAdapterCardClass,
  sessionLaunchSectionClass,
  sessionLaunchSelectorGridClass,
  AgentHarnessSelector,
  SessionLaunchLoadingSelect,
  SessionLaunchPreviewSelectors,
} from "./launch-form-selectors.tsx"
import { filterSlashCommandSuggestions, type SessionLaunchFormState } from "./launch-form-state.ts"

export function SessionLaunchForm(props: {
  form: SessionLaunchFormState
  onSubmit: () => Promise<void> | void
  projects: readonly ProjectRecord[]
}) {
  const { form } = props
  const projectItems: SessionInputSelectItem[] = props.projects.map((project) => ({
    value: project.path,
    label: project.name,
    detail: project.path,
    searchText: project.path,
    icon: Folder,
  }))
  const selectedAdapter = form.selectedAdapter.value

  return (
    <div class={sessionLaunchSectionClass}>
      <div class={sessionLaunchSelectorGridClass}>
        <SessionInputSelect
          disabled={props.projects.length === 0}
          filterable={true}
          items={projectItems}
          label="Project"
          menuLabel="Projects"
          open={form.openPicker.value === "project"}
          placeholder="Select a project"
          value={form.draftProjectPath.value}
          onOpenChange={(open) => {
            form.setOpenPicker(open ? "project" : null)
          }}
          onValueChange={(value) => {
            form.draftProjectPath.value = value
            form.launchPreview.value = null
            form.setOpenPicker(null)
          }}
        />
        <Suspense fallback={<SessionLaunchLoadingSelect label="Adapter" />}>
          <AgentHarnessSelector form={form} />
        </Suspense>
      </div>

      {selectedAdapter ? (
        <div class={sessionLaunchAdapterCardClass}>
          <p
            class={css({
              color: "text",
              fontSize: "0.9rem",
              fontWeight: "600",
            })}
          >
            <span class={css({ color: "muted", fontWeight: "560" })}>
              Adapter
              {" · "}
            </span>
            <span>{selectedAdapter.name}</span>
            <span class={css({ color: "muted", fontWeight: "560" })}>
              {" · "}
              {selectedAdapter.id}
              {selectedAdapter.unofficial ? " · Unofficial" : ""}
            </span>
          </p>
          {selectedAdapter.description ? (
            <p
              class={css({
                color: "muted",
                fontSize: "0.88rem",
                lineHeight: "1.5",
              })}
            >
              {selectedAdapter.description}
            </p>
          ) : null}
        </div>
      ) : null}

      <Suspense
        fallback={
          <div class={sessionLaunchSectionClass}>
            <div class={sessionLaunchSelectorGridClass}>
              <SessionLaunchLoadingSelect label="Launch location" />
              <SessionLaunchLoadingSelect label="Git branch" />
            </div>
            <div class={sessionLaunchSelectorGridClass}>
              <SessionLaunchLoadingSelect label="Model" />
              <SessionLaunchLoadingSelect label="Thinking level" />
            </div>
          </div>
        }
      >
        <SessionLaunchPreviewSelectors form={form} />
      </Suspense>

      <Composer
        helperText="Enter launches, Shift+Enter inserts a newline, Mod+Enter launches from the dialog, and @, $, or / open suggestions."
        loadSuggestions={async (input) => {
          const cwd = form.draftProjectPath.value

          if (!cwd) {
            return []
          }

          if (input.trigger === "slash") {
            return filterSlashCommandSuggestions(
              form.launchPreview.value?.slashCommands ?? [],
              input.query,
            )
          }

          const response = await goddardSdk.session.draftSuggestions({
            cwd,
            trigger: input.trigger,
            query: input.query,
          })

          return response.suggestions
        }}
        onPromptChange={(prompt) => {
          form.draftPromptBlocks.value = prompt
        }}
        placeholder="Describe the first thing this session should do."
        submitLabel="Launch session"
        onSubmit={async () => {
          await props.onSubmit()
        }}
      />
    </div>
  )
}
