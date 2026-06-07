import { useEffect } from "preact/hooks"
import { Folder } from "lucide-react"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { AppCommand } from "~/commands/app-command.ts"
import { getShortcutLabel } from "~/commands/shortcut-label.ts"
import type { ProjectRecord } from "~/projects/project-registry.ts"
import { goddardSdk } from "~/sdk.ts"
import composerInputStyles from "~/session-input/composer-layout.style.ts"
import { SessionInput, type SessionInputClasses } from "~/session-input/input.tsrx"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import selectorStyles from "./launch-form-selectors.style.ts"
import {
  AgentHarnessSelector,
  SessionLaunchLoadingSelect,
  SessionLaunchPreviewLoadingSelectors,
  SessionLaunchPreviewSelectors,
  SessionSubpackageSelector,
} from "./launch-form-selectors.tsrx"
import { filterSlashCommandSuggestions, type SessionLaunchFormState } from "./launch-form-state.ts"

const launchInputClasses: SessionInputClasses = {
  form: composerInputStyles.form,
  editorFrame: composerInputStyles.editorFrame,
  contentEditable: composerInputStyles.contentEditable,
  placeholder: composerInputStyles.placeholder,
  footer: composerInputStyles.footer,
  footerControls: composerInputStyles.footerControls,
  helperText: composerInputStyles.helperText,
  contextUsage: composerInputStyles.contextUsage,
  submitButton: composerInputStyles.submitButton,
}

export component SessionLaunchForm(props: {
  form: SessionLaunchFormState;
  onEscape?: () => void;
  showProjectSelector?: boolean;
  onSubmit: () => Promise<void> | void;
  projects: readonly ProjectRecord[];
}) {
  const { form } = props
  const shortcutRegistry = useShortcutRegistry()
  const showProjectSelector = props.showProjectSelector ?? true
  const projectItems: SessionInputSelectItem[] = props.projects.map(
    (project) => ({
      value: project.path,
      label: project.name,
      detail: project.path,
      Icon: Folder,
      searchText: project.path,
    }),
  )
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
    form.draftSubpackagePath.value = null
    form.launchPreview.value = null
  }, [form, props.projects])

  <div class={selectorStyles.section}>
    <div class={selectorStyles.grid}>
      if (showProjectSelector) {
        <SessionInputSelect
          disabled={props.projects.length === 0}
          filterable={true}
          items={projectItems}
          label="Project"
          open={form.getPickerOpen("project")}
          placeholder="Select a project"
          shortcut={projectShortcut}
          value={form.draftProjectPath.value}
          onValueChange={(value) => {
            form.draftProjectPath.value = value
            form.draftSubpackagePath.value = null
            form.launchPreview.value = null
            form.setOpenPicker(null)
          }}
        />
      }

      try {
        <SessionSubpackageSelector {form} />
      } pending {
        <SessionLaunchLoadingSelect label="Working directory" />
      }
      try {
        <AgentHarnessSelector {form} />
      } pending {
        <SessionLaunchLoadingSelect label="Adapter" />
      }
    </div>

    <SessionInput
      autoFocus={true}
      classes={launchInputClasses}
      footerInsideFrame={true}
      footerControls={<tsrx>
        try {
          <SessionLaunchPreviewSelectors compact={true} {form} />
        } pending {
          <SessionLaunchPreviewLoadingSelectors compact={true} />
        }
      </tsrx>}
      helperText={null}
      initialPrompt={form.draftPromptBlocks.value}
      loadSuggestions={async (input) => {
        const cwd = form.effectiveCwd.value

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
      onEscape={props.onEscape}
      iconOnlySubmit={true}
      placeholder="Describe the first thing this session should do."
      submitLabel="Launch session"
      onSubmit={async () => {
        await props.onSubmit()
      }}
    />
  </div>
}
