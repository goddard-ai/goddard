import { Suspense } from "preact/compat"
import { useEffect } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { AppCommand } from "~/commands/app-command.ts"
import type { ProjectRecord } from "~/projects/project-registry.ts"
import { goddardSdk } from "~/sdk.ts"
import { SessionInput, type SessionInputClasses } from "~/session-input/input.tsrx"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import selectorStyles from "./launch-form-selectors.style.ts"
import {
  AgentHarnessSelector,
  SessionLaunchLoadingSelect,
  SessionLaunchPreviewSelectors,
} from "./launch-form-selectors.tsrx"
import { filterSlashCommandSuggestions, type SessionLaunchFormState } from "./launch-form-state.ts"
import styles from "./launch-form.style.ts"

const launchInputClasses: SessionInputClasses = {
  form: styles.form,
  editorFrame: styles.editorFrame,
  contentEditable: styles.contentEditable,
  placeholder: styles.placeholder,
  footer: styles.footer,
  helperText: styles.helperText,
  submitButton: styles.submitButton,
}

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

export component SessionLaunchForm(props: {
  form: SessionLaunchFormState;
  onEscape?: () => void;
  onSubmit: () => Promise<void> | void;
  projects: readonly ProjectRecord[];
}) {
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const projectItems: SessionInputSelectItem[] = props.projects.map(
    (project) => ({
      value: project.path,
      label: project.name,
      detail: project.path,
      searchText: project.path,
    }),
  )
  const selectedAdapter = form.selectedAdapter.value
  const projectShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openProjectSelector.id]?.[0],
  )

  useEffect(() => {
    const nextProjectPath = props.projects.some(
      (project) => project.path === form.draftProjectPath.value,
    )
      ? form.draftProjectPath.value
      : props.projects[0]?.path ?? null

    if (nextProjectPath === form.draftProjectPath.value) {
      return
    }

    form.draftProjectPath.value = nextProjectPath
    form.launchPreview.value = null
  }, [form, props.projects])

  <div class={selectorStyles.section}>
    <div class={selectorStyles.grid}>
      <SessionInputSelect
        disabled={props.projects.length === 0}
        filterable={true}
        items={projectItems}
        label="Project"
        open={form.openPicker.value === "project"}
        placeholder="Select a project"
        shortcut={projectShortcut}
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
      <Suspense fallback={<><SessionLaunchLoadingSelect label="Adapter" /></>}>
        <AgentHarnessSelector {form} />
      </Suspense>
    </div>
    if (selectedAdapter) {
      <div class={selectorStyles.adapterCard}>
        <p class={styles.adapterMeta}>
          <span class={styles.mutedMeta}>
            Adapter
            {" · "}
          </span>
          <span>
            {selectedAdapter.name}
          </span>
          <span class={styles.mutedMeta}>
            {" · "}
            {selectedAdapter.id}
            {selectedAdapter.unofficial ? " · Unofficial" : ""}
          </span>
        </p>
        if (selectedAdapter.description) {
          <p class={styles.adapterDescription}>
            {selectedAdapter.description}
          </p>
        }
      </div>
    }
    <Suspense
      fallback={<>
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
      </>}
    >
      <SessionLaunchPreviewSelectors {form} />
    </Suspense>
    <SessionInput
      autoFocus={true}
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

        const response = await goddardSdk.session.draftSuggestions(
          {
            cwd,
            trigger: input.trigger,
            query: input.query,
          },
        )

        return response.suggestions
      }}
      onPromptChange={(prompt) => {
        form.draftPromptBlocks.value = prompt
      }}
      onEscape={props.onEscape}
      placeholder="Describe the first thing this session should do."
      submitLabel="Launch session"
      onSubmit={async () => {
        await props.onSubmit()
      }}
    />
  </div>
}
