import { Dialog, type UseDialogReturn } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css, cx } from "@goddard-ai/styled-system/css"
import { useModel } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { createSession } from "./actions.ts"
import { SessionLaunchForm, SessionLaunchFormState } from "./launch-form.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"

export default function SessionLaunchDialog(props: { dialog: UseDialogReturn }) {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const form = useModel(SessionLaunchFormState)

  useEffect(() => {
    if (props.dialog.open) {
      form.reset()
    }
  }, [form, props.dialog.open])

  return (
    <Portal>
      <Dialog.Backdrop
        class={css({
          position: "fixed",
          inset: "0",
          backgroundColor: "overlay",
          backdropFilter: "blur(6px)",
          opacity: "1",
          transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          zIndex: "60",
          "@starting-style": {
            opacity: "0",
          },
        })}
      />
      <Dialog.Positioner
        class={css({
          position: "fixed",
          inset: "0",
          display: "grid",
          placeItems: "center",
          padding: "16px",
          zIndex: "61",
        })}
      >
        <Dialog.Content
          class={css({
            width: "min(640px, calc(100vw - 32px))",
            maxHeight: "calc(100vh - 32px)",
            display: "flex",
            flexDirection: "column",
            gap: "18px",
            overflowY: "auto",
            padding: "20px",
            borderRadius: "18px",
            border: "1px solid",
            borderColor: "border",
            backgroundColor: "panel",
            opacity: "1",
            transform: "translateY(0)",
            transition:
              "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
            outline: "none",
            "@starting-style": {
              opacity: "0",
              transform: "translateY(8px)",
            },
          })}
        >
          <header
            class={css({
              display: "flex",
              alignItems: "flex-start",
              justifyContent: "space-between",
              gap: "16px",
            })}
          >
            <div class={css({ display: "grid", gap: "6px" })}>
              <Dialog.Title
                class={css({
                  color: "text",
                  fontSize: "1.2rem",
                  fontWeight: "700",
                  lineHeight: "1.25",
                })}
              >
                Launch session
              </Dialog.Title>
              <Dialog.Description
                class={css({
                  color: "muted",
                  fontSize: "0.9rem",
                  lineHeight: "1.6",
                })}
              >
                Choose a project, adapter, and first prompt. The new chat tab opens immediately
                after creation.
              </Dialog.Description>
            </div>
            <Dialog.CloseTrigger asChild>
              <button
                class={cx(
                  css({
                    display: "grid",
                    placeItems: "center",
                    width: "32px",
                    height: "32px",
                    borderRadius: "10px",
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
          </header>
          <SessionLaunchForm
            form={form}
            onSubmit={async () => {
              const sessionInput = form.sessionInput.value

              if (!sessionInput) {
                return
              }

              try {
                const { session } = await createSession(sessionInput)
                props.dialog.setOpen(false)
                form.reset()
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
            }}
            projects={projectRegistry.projectList}
          />
        </Dialog.Content>
      </Dialog.Positioner>
    </Portal>
  )
}
