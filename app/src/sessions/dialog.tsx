import { Dialog } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import type { AdapterCatalogEntry, CreateSessionRequest } from "@goddard-ai/sdk"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Sparkles, X } from "lucide-react"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { createSession } from "./actions.ts"
import { LaunchForm } from "./launch-form.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"

export function SessionLaunchDialog(props: {
  adapters: readonly AdapterCatalogEntry[]
  canSubmit: boolean
  createSessionInput: () => CreateSessionRequest | null
  draftAdapterId: string | null
  draftProjectPath: string | null
  draftPrompt: string
  isDialogOpen: boolean
  onChangeAdapterId: (adapterId: string | null) => void
  onChangeProjectPath: (projectPath: string | null) => void
  onChangePrompt: (prompt: string) => void
  onClose: () => void
}) {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
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

  function closeDialog() {
    props.onClose()
  }

  async function launchSession() {
    const sessionInput = props.createSessionInput()

    if (!sessionInput) {
      return
    }

    try {
      const { session } = await createSession(sessionInput)
      props.onClose()
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
      open={props.isDialogOpen}
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
              adapters={props.adapters}
              canSubmit={props.canSubmit}
              draftAdapterId={props.draftAdapterId}
              draftProjectPath={props.draftProjectPath}
              draftPrompt={props.draftPrompt}
              onChangeAdapterId={props.onChangeAdapterId}
              onChangeProjectPath={props.onChangeProjectPath}
              onChangePrompt={props.onChangePrompt}
              onSubmit={launchSession}
              projects={projectRegistry.projectList}
            />
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  )
}
