import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Folder } from "lucide-react"
import { Suspense } from "preact/compat"
import { useEffect } from "preact/hooks"

import type { ProjectRecord } from "~/projects/project-registry.ts"
import { goddardSdk } from "~/sdk.ts"
import { SessionInput, type SessionInputClasses } from "~/session-input/input.tsx"
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

const launchInputClasses = {
  form: css({
    display: "grid",
    gap: "12px",
  }),
  editorFrame: css({
    position: "relative",
    borderRadius: "14px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
    transition:
      "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _focusWithin: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent)`,
    },
  }),
  contentEditable: css({
    width: "100%",
    minHeight: "124px",
    padding: "12px 14px",
    color: "text",
    fontSize: "0.9rem",
    lineHeight: "1.55",
    outline: "none",
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
  }),
  placeholder: css({
    position: "absolute",
    inset: "12px 14px auto",
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.55",
    pointerEvents: "none",
  }),
  footer: css({
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "12px",
  }),
  helperText: css({
    color: "muted",
    fontSize: "0.82rem",
    lineHeight: "1.55",
  }),
  submitButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "8px",
    minWidth: "124px",
    height: "40px",
    paddingInline: "14px",
    borderRadius: "12px",
    border: "1px solid",
    borderColor: "accent",
    backgroundColor: "surface",
    color: "text",
    fontSize: "0.88rem",
    fontWeight: "640",
    cursor: "pointer",
    _disabled: {
      cursor: "not-allowed",
      opacity: "0.52",
    },
  }),
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
