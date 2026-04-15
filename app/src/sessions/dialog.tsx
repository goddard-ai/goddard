import { Dialog, type UseDialogReturn } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css, cx } from "@goddard-ai/styled-system/css"
import { useModel } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { createSession } from "./actions.ts"
import { SessionLaunchFormState } from "./launch-form-state.ts"
import { SessionLaunchForm } from "./launch-form.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"

export default function SessionLaunchDialog(props: { dialog: UseDialogReturn }) {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const form = useModel(SessionLaunchFormState)
  const hasModelSelector =
    (form.launchPreview.value?.models?.availableModels.length ?? 0) > 0 && props.dialog.open
  const hasProjectSelector = projectRegistry.projectList.length > 0 && props.dialog.open
  const hasThinkingLevel = form.thinkingOption.value !== null && props.dialog.open
  const canSubmit = form.canSubmit.value && props.dialog.open

  async function launchSession() {
    const sessionInput = form.sessionInput.value

    if (!sessionInput) {
      return
    }

    try {
      const projectPath = form.draftProjectPath.value
      const { session } = await createSession(sessionInput)
      props.dialog.setOpen(false)
      form.reset()
      workbenchTabSet.openOrFocusTab({
        id: `session:${session.id}`,
        kind: "sessionChat",
        title: getSessionDisplayTitle(session),
        payload: {
          projectPath,
          sessionId: session.id,
        },
        dirty: false,
      })
    } catch (error) {
      console.error("Failed to create session.", error)
    }
  }

  useEffect(() => {
    if (props.dialog.open) {
      form.reset(projectContext.activeProjectPath)
    }
  }, [form, projectContext, props.dialog.open])

  useEffect(() => {
    commandContext.sessionInputActive.value = props.dialog.open
    commandContext.sessionInputCanSubmit.value = canSubmit
    commandContext.sessionInputHasModelSelector.value = hasModelSelector
    commandContext.sessionInputHasProjectSelector.value = hasProjectSelector
    commandContext.sessionInputHasThinkingLevel.value = hasThinkingLevel

    return () => {
      commandContext.sessionInputActive.value = false
      commandContext.sessionInputCanSubmit.value = false
      commandContext.sessionInputHasModelSelector.value = false
      commandContext.sessionInputHasProjectSelector.value = false
      commandContext.sessionInputHasThinkingLevel.value = false
    }
  }, [canSubmit, hasModelSelector, hasProjectSelector, hasThinkingLevel, props.dialog.open])

  useAppCommand(AppCommand.sessionInput.openProjectSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("project")
  })

  useAppCommand(AppCommand.sessionInput.openModelSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("model")
  })

  useAppCommand(AppCommand.sessionInput.toggleThinkingLevel, () => {
    if (!props.dialog.open) {
      return
    }

    form.toggleThinkingLevel()
  })

  useAppCommand(AppCommand.sessionInput.submit, () => {
    if (!props.dialog.open) {
      return
    }

    void launchSession()
  })

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
            width: "min(880px, calc(100vw - 32px))",
            maxHeight: "calc(100vh - 32px)",
            display: "flex",
            flexDirection: "column",
            gap: "16px",
            overflowY: "auto",
            padding: "18px",
            borderRadius: "20px",
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
          <Dialog.Title
            class={css({
              position: "absolute",
              width: "1px",
              height: "1px",
              padding: "0",
              margin: "-1px",
              overflow: "hidden",
              clip: "rect(0, 0, 0, 0)",
              whiteSpace: "nowrap",
              border: "0",
            })}
          >
            Launch session
          </Dialog.Title>
          <Dialog.CloseTrigger asChild>
            <button
              class={cx(
                css({
                  position: "absolute",
                  top: "12px",
                  right: "12px",
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
          <SessionLaunchForm
            form={form}
            onSubmit={launchSession}
            projects={projectRegistry.projectList}
          />
        </Dialog.Content>
      </Dialog.Positioner>
    </Portal>
  )
}
