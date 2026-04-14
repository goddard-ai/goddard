import { type ListAdaptersResponse } from "@goddard-ai/sdk"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { computed, createModel, signal } from "@preact/signals"
import { Suspense } from "preact/compat"

import { useQuery } from "~/lib/query.ts"
import type { ProjectRecord } from "~/projects/project-registry.ts"
import { goddardSdk } from "~/sdk.ts"

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

/** Session launch form state shared across the dialog and form body. */
export const SessionLaunchFormState = createModel(function () {
  const draftAdapterId = signal<string | null>(null)
  const draftProjectPath = signal<string | null>(null)
  const draftPrompt = signal("")
  const adapterCatalog = signal<ListAdaptersResponse | null>(null)

  const sessionInput = computed(() => {
    const agent = draftAdapterId.value
    const cwd = draftProjectPath.value
    const initialPrompt = draftPrompt.value

    if (!agent || !cwd || !initialPrompt) {
      return null
    }

    return {
      agent,
      cwd,
      mcpServers: [],
      systemPrompt: "",
      initialPrompt,
    }
  })
  const selectedAdapter = computed(
    () =>
      adapterCatalog.value?.adapters.find((adapter) => adapter.id === draftAdapterId.value) ?? null,
  )

  function syncAdapterSelection(nextAdapterCatalog: ListAdaptersResponse | null) {
    if (!nextAdapterCatalog) {
      draftAdapterId.value = null
      return
    }

    const availableAdapterIds = new Set(nextAdapterCatalog.adapters.map((adapter) => adapter.id))
    const nextAdapterId =
      draftAdapterId.value && availableAdapterIds.has(draftAdapterId.value)
        ? draftAdapterId.value
        : nextAdapterCatalog.defaultAdapterId &&
            availableAdapterIds.has(nextAdapterCatalog.defaultAdapterId)
          ? nextAdapterCatalog.defaultAdapterId
          : (nextAdapterCatalog.adapters[0]?.id ?? null)

    if (draftAdapterId.value !== nextAdapterId) {
      draftAdapterId.value = nextAdapterId
    }
  }

  adapterCatalog.subscribe(syncAdapterSelection)

  return {
    adapterCatalog,
    canSubmit: computed(() => sessionInput.value !== null),
    draftAdapterId,
    draftProjectPath,
    draftPrompt,
    reset(preferredProjectPath: string | null = null) {
      const previousProjectPath = draftProjectPath.value
      draftAdapterId.value = null
      draftProjectPath.value = preferredProjectPath
      draftPrompt.value = ""

      if (preferredProjectPath === previousProjectPath) {
        syncAdapterSelection(adapterCatalog.value)
      }
    },
    selectedAdapter,
    sessionInput,
  }
})

/** One live model instance for the session launch dialog form. */
export type SessionLaunchFormState = InstanceType<typeof SessionLaunchFormState>

/** Resolves the adapter list for the selected project and writes it into the form model. */
function AdapterSelect(props: { form: SessionLaunchFormState }) {
  const { form } = props
  const adapterCatalog = useQuery(goddardSdk.adapter.list, [
    { cwd: form.draftProjectPath.value ?? undefined },
  ])

  form.adapterCatalog.value = adapterCatalog

  return (
    <select
      class={cx(
        controlClass,
        css({
          height: "48px",
          paddingInline: "16px",
        }),
      )}
      value={form.draftAdapterId.value ?? ""}
      onInput={(event) => {
        form.draftAdapterId.value = event.currentTarget.value || null
      }}
    >
      <option value="">Select an adapter</option>
      {adapterCatalog.adapters.map((adapter) => (
        <option key={adapter.id} value={adapter.id}>
          {adapter.name}
          {adapter.unofficial ? " (Unofficial)" : ""}
          {` · ${adapter.version}`}
        </option>
      ))}
    </select>
  )
}

/** Renders the session launch form around one shared form model object. */
export function SessionLaunchForm(props: {
  form: SessionLaunchFormState
  onSubmit: () => Promise<void> | void
  projects: readonly ProjectRecord[]
}) {
  const { form } = props
  const selectedAdapter = form.selectedAdapter.value

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
          value={form.draftProjectPath.value ?? ""}
          onInput={(event) => {
            form.draftProjectPath.value = event.currentTarget.value || null
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
        <Suspense
          fallback={
            <select
              class={cx(
                controlClass,
                css({
                  height: "48px",
                  paddingInline: "16px",
                }),
              )}
              disabled
            >
              <option>Loading adapters...</option>
            </select>
          }
        >
          <AdapterSelect form={form} />
        </Suspense>
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
          value={form.draftPrompt.value}
          onInput={(event) => {
            form.draftPrompt.value = event.currentTarget.value
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
          disabled={!form.canSubmit.value}
          type="submit"
        >
          Launch session
        </button>
      </div>
    </form>
  )
}
