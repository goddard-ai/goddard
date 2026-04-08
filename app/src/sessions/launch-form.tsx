import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { ProjectRecord } from "~/projects/project-registry.ts"

const fieldClass = css({
  display: "grid",
  gap: "8px",
})

const labelClass = css({
  color: "text",
  fontSize: "0.82rem",
  fontWeight: "680",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

const controlClass = css({
  width: "100%",
  borderRadius: "16px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "text",
  fontSize: "0.95rem",
  outline: "none",
  transition:
    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    borderColor: "accentStrong",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
  },
})

export function LaunchForm(props: {
  canSubmit: boolean
  draftProjectPath: string | null
  draftPrompt: string
  onChangeProjectPath: (projectPath: string | null) => void
  onChangePrompt: (prompt: string) => void
  onSubmit: () => Promise<void> | void
  projects: readonly ProjectRecord[]
}) {
  return (
    <form
      class={css({
        display: "grid",
        gap: "18px",
      })}
      onSubmit={(event) => {
        event.preventDefault()
        void props.onSubmit()
      }}
    >
      <label class={fieldClass}>
        <span class={labelClass}>Project</span>
        <select
          class={cx(
            controlClass,
            css({
              height: "48px",
              paddingInline: "16px",
            }),
          )}
          value={props.draftProjectPath ?? ""}
          onInput={(event) => {
            props.onChangeProjectPath(event.currentTarget.value || null)
          }}
        >
          <option value="">Select a project</option>
          {props.projects.map((project) => (
            <option key={project.path} value={project.path}>
              {project.name}
            </option>
          ))}
        </select>
      </label>
      <label class={fieldClass}>
        <span class={labelClass}>Launch prompt</span>
        <textarea
          class={cx(
            controlClass,
            css({
              minHeight: "136px",
              padding: "14px 16px",
              resize: "vertical",
              lineHeight: "1.6",
            }),
          )}
          placeholder="Describe the first thing this session should do."
          value={props.draftPrompt}
          onInput={(event) => {
            props.onChangePrompt(event.currentTarget.value)
          }}
        />
      </label>
      <div
        class={css({
          display: "flex",
          justifyContent: "flex-end",
        })}
      >
        <button
          class={css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            minWidth: "128px",
            height: "42px",
            paddingInline: "16px",
            borderRadius: "14px",
            border: "1px solid",
            borderColor: "accent",
            background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
            color: "text",
            fontSize: "0.9rem",
            fontWeight: "680",
            cursor: "pointer",
            _disabled: {
              cursor: "not-allowed",
              opacity: "0.52",
            },
          })}
          disabled={!props.canSubmit}
          type="submit"
        >
          Launch session
        </button>
      </div>
    </form>
  )
}
