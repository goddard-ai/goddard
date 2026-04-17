import { Folder } from "lucide-react"
import { Suspense } from "preact/compat"
import { useEffect } from "preact/hooks"

import type { ProjectRecord } from "~/projects/project-registry.ts"
import { goddardSdk } from "~/sdk.ts"
import { SessionInput, type SessionInputClasses } from "~/session-input/input.tsx"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsx"
import selectorStyles from "./launch-form-selectors.style.ts"
import {
  AgentHarnessSelector,
  SessionLaunchLoadingSelect,
  SessionLaunchPreviewSelectors,
} from "./launch-form-selectors.tsx"
import { filterSlashCommandSuggestions, type SessionLaunchFormState } from "./launch-form-state.ts"
import styles from "./launch-form.style.ts"

const launchInputClasses = {
  form: styles.form,
  editorFrame: styles.editorFrame,
  contentEditable: styles.contentEditable,
  placeholder: styles.placeholder,
  footer: styles.footer,
  helperText: styles.helperText,
  submitButton: styles.submitButton,
} satisfies SessionInputClasses

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

  useEffect(() => {
    const nextProjectPath = props.projects.some(
      (project) => project.path === form.draftProjectPath.value,
    )
      ? form.draftProjectPath.value
      : (props.projects[0]?.path ?? null)

    if (nextProjectPath === form.draftProjectPath.value) {
      return
    }

    form.draftProjectPath.value = nextProjectPath
    form.launchPreview.value = null
  }, [form, props.projects])

  return (
    <div class={selectorStyles.section}>
      <div class={selectorStyles.grid}>
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
        <div class={selectorStyles.adapterCard}>
          <p class={styles.adapterMeta}>
            <span class={styles.mutedMeta}>
              Adapter
              {" · "}
            </span>
            <span>{selectedAdapter.name}</span>
            <span class={styles.mutedMeta}>
              {" · "}
              {selectedAdapter.id}
              {selectedAdapter.unofficial ? " · Unofficial" : ""}
            </span>
          </p>
          {selectedAdapter.description ? (
            <p class={styles.adapterDescription}>{selectedAdapter.description}</p>
          ) : null}
        </div>
      ) : null}

      <Suspense
        fallback={
          <div class={selectorStyles.section}>
            <div class={selectorStyles.grid}>
              <SessionLaunchLoadingSelect label="Launch location" />
              <SessionLaunchLoadingSelect label="Git branch" />
            </div>
            <div class={selectorStyles.grid}>
              <SessionLaunchLoadingSelect label="Model" />
              <SessionLaunchLoadingSelect label="Thinking level" />
            </div>
          </div>
        }
      >
        <SessionLaunchPreviewSelectors form={form} />
      </Suspense>

      <SessionInput
        classes={launchInputClasses}
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
