import { css, cx } from "@goddard-ai/styled-system/css"
import type { AdapterCatalogEntry } from "@goddard-ai/sdk"
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
  adapters: readonly AdapterCatalogEntry[]
  canSubmit: boolean
  draftAdapterId: string | null
  draftProjectPath: string | null
  draftPrompt: string
  onChangeAdapterId: (adapterId: string | null) => void
  onChangeProjectPath: (projectPath: string | null) => void
  onChangePrompt: (prompt: string) => void
  onSubmit: () => Promise<void> | void
  projects: readonly ProjectRecord[]
}) {
  const selectedAdapter =
    props.adapters.find((adapter) => adapter.id === props.draftAdapterId) ?? null

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
        <span class={labelClass}>Adapter</span>
        <select
          class={cx(
            controlClass,
            css({
              height: "48px",
              paddingInline: "16px",
            }),
          )}
          value={props.draftAdapterId ?? ""}
          onInput={(event) => {
            props.onChangeAdapterId(event.currentTarget.value || null)
          }}
        >
          <option value="">Select an adapter</option>
          {props.adapters.map((adapter) => (
            <option key={adapter.id} value={adapter.id}>
              {adapter.name}
              {adapter.unofficial ? " (Unofficial)" : ""}
              {` · ${adapter.version}`}
            </option>
          ))}
        </select>
      </label>
      {selectedAdapter ? (
        <div
          class={css({
            display: "grid",
            gap: "6px",
            padding: "14px 16px",
            borderRadius: "18px",
            border: "1px solid",
            borderColor: "border",
            backgroundColor: "surface",
          })}
        >
          <div
            class={css({
              display: "flex",
              alignItems: "center",
              gap: "10px",
              color: "text",
              fontSize: "0.92rem",
              fontWeight: "650",
            })}
          >
            {selectedAdapter.icon ? (
              <img
                alt=""
                class={css({
                  width: "16px",
                  height: "16px",
                  borderRadius: "4px",
                })}
                src={selectedAdapter.icon}
              />
            ) : null}
            <span>{selectedAdapter.name}</span>
            <span class={css({ color: "muted", fontWeight: "560" })}>
              {selectedAdapter.id}
              {selectedAdapter.unofficial ? " (Unofficial)" : ""}
            </span>
          </div>
          <p
            class={css({
              color: "muted",
              fontSize: "0.88rem",
              lineHeight: "1.5",
            })}
          >
            {selectedAdapter.description}
          </p>
        </div>
      ) : null}
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
