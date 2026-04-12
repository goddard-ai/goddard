import { Dialog } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { Sparkles, X } from "lucide-react"
import { useListener } from "preact-sigma"
import { useEffect } from "preact/hooks"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { createSession } from "./actions.ts"
import { LaunchForm } from "./launch-form.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"
import { buildCreateSessionInput } from "./session-launch.ts"

export function SessionLaunchDialog() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const isDialogOpen = useSignal(false)
  const draftAdapterId = useSignal<string | null>(null)
  const draftProjectPath = useSignal<string | null>(null)
  const draftPrompt = useSignal("")
  const adapterCatalog = useQuery(goddardSdk.adapter.list, [
    { cwd: draftProjectPath.value ?? undefined },
  ])
  const overlayClass = css({
    position: "fixed",
    inset: "0",
    background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 10%, rgba(23, 29, 36, 0.24)), rgba(23, 29, 36, 0.34))`,
    backdropFilter: "blur(10px)",
    opacity: "1",
    transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
    "@starting-style": {
      opacity: "0",
    },
  })
  const positionerClass = css({
    position: "fixed",
    inset: "0",
    display: "grid",
    placeItems: "center",
    padding: "16px",
  })
  const contentClass = css({
    width: "min(640px, calc(100vw - 32px))",
    maxHeight: "calc(100vh - 32px)",
    overflowY: "auto",
    padding: "28px",
    borderRadius: "30px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
    boxShadow: "0 34px 90px rgba(84, 102, 124, 0.2)",
    opacity: "1",
    transform: "translateY(0) scale(1)",
    transition:
      "opacity 220ms cubic-bezier(0.23, 1, 0.32, 1), transform 220ms cubic-bezier(0.23, 1, 0.32, 1)",
    outline: "none",
    "@starting-style": {
      opacity: "0",
      transform: "translateY(16px) scale(0.985)",
    },
  })

  useEffect(() => {
    const availableAdapterIds = new Set(adapterCatalog.adapters.map((adapter) => adapter.id))
    const nextAdapterId =
      draftAdapterId.value && availableAdapterIds.has(draftAdapterId.value)
        ? draftAdapterId.value
        : adapterCatalog.defaultAdapterId &&
            availableAdapterIds.has(adapterCatalog.defaultAdapterId)
          ? adapterCatalog.defaultAdapterId
          : (adapterCatalog.adapters[0]?.id ?? null)

    if (draftAdapterId.value !== nextAdapterId) {
      draftAdapterId.value = nextAdapterId
    }
  }, [adapterCatalog.adapters, adapterCatalog.defaultAdapterId, draftAdapterId.value])

  useListener(globalEventHub, "sessionLaunchDialogRequested", (detail) => {
    isDialogOpen.value = true
    draftAdapterId.value = null
    draftProjectPath.value = detail.preferredProjectPath
    draftPrompt.value = ""
  })

  function closeDialog() {
    isDialogOpen.value = false
    draftAdapterId.value = null
    draftProjectPath.value = null
    draftPrompt.value = ""
  }

  function setDraftAdapterId(adapterId: string | null) {
    draftAdapterId.value = adapterId
  }

  function setDraftProjectPath(projectPath: string | null) {
    draftProjectPath.value = projectPath
  }

  function setDraftPrompt(prompt: string) {
    draftPrompt.value = prompt
  }

  function createSessionInput() {
    return buildCreateSessionInput(draftProjectPath.value, draftAdapterId.value, draftPrompt.value)
  }

  function canSubmit() {
    return createSessionInput() !== null
  }

  async function launchSession() {
    const sessionInput = createSessionInput()

    if (!sessionInput) {
      return
    }

    try {
      const { session } = await createSession(sessionInput)
      closeDialog()
      workbenchTabSet.openOrFocusTab({
        id: `session:${session.id}`,
        kind: "sessionChat",
        title: getSessionDisplayTitle(session),
        payload: {
          sessionId: session.id,
        },
        dirty: false,
      })
    } catch (error) {
      console.error("Failed to create session.", error)
    }
  }

  return (
    <Dialog.Root
      open={isDialogOpen.value}
      onOpenChange={(details: { open: boolean }) => {
        if (!details.open) {
          closeDialog()
        }
      }}
    >
      <Portal>
        <Dialog.Backdrop class={overlayClass} />
        <Dialog.Positioner class={positionerClass}>
          <Dialog.Content class={contentClass}>
            <div
              class={css({
                display: "flex",
                alignItems: "flex-start",
                justifyContent: "space-between",
                gap: "16px",
                marginBottom: "22px",
              })}
            >
              <div class={css({ display: "grid", gap: "10px" })}>
                <span
                  class={css({
                    display: "inline-flex",
                    alignItems: "center",
                    gap: "8px",
                    width: "fit-content",
                    padding: "8px 12px",
                    borderRadius: "999px",
                    backgroundColor: "surface",
                    color: "accentStrong",
                    fontSize: "0.72rem",
                    fontWeight: "700",
                    letterSpacing: "0.14em",
                    textTransform: "uppercase",
                  })}
                >
                  <Sparkles size={14} strokeWidth={2} />
                  New session
                </span>
                <div class={css({ display: "grid", gap: "8px" })}>
                  <Dialog.Title
                    class={css({
                      color: "text",
                      fontSize: "1.45rem",
                      fontWeight: "760",
                      letterSpacing: "-0.03em",
                      lineHeight: "1.1",
                    })}
                  >
                    Launch a local session
                  </Dialog.Title>
                  <Dialog.Description
                    class={css({
                      color: "muted",
                      fontSize: "0.94rem",
                      lineHeight: "1.7",
                    })}
                  >
                    This minimal flow creates a session record, seeds the transcript, and opens the
                    chat tab immediately from the shared daemon session data.
                  </Dialog.Description>
                </div>
              </div>
              <Dialog.CloseTrigger asChild>
                <button
                  class={cx(
                    css({
                      display: "grid",
                      placeItems: "center",
                      width: "36px",
                      height: "36px",
                      borderRadius: "12px",
                      border: "1px solid",
                      borderColor: "border",
                      backgroundColor: "background",
                      color: "muted",
                      cursor: "pointer",
                    }),
                  )}
                  type="button"
                >
                  <X size={16} strokeWidth={2.2} />
                </button>
              </Dialog.CloseTrigger>
            </div>
            <LaunchForm
              adapters={adapterCatalog.adapters}
              canSubmit={canSubmit()}
              draftAdapterId={draftAdapterId.value}
              draftProjectPath={draftProjectPath.value}
              draftPrompt={draftPrompt.value}
              onChangeAdapterId={setDraftAdapterId}
              onChangeProjectPath={setDraftProjectPath}
              onChangePrompt={setDraftPrompt}
              onSubmit={launchSession}
              projects={projectRegistry.projectList}
            />
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  )
}
