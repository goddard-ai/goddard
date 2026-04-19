import { Dialog, type UseDialogReturn } from "@ark-ui/react/dialog"
import { useModel } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { DialogPortal } from "~/lib/dialog-portal.tsx"
import { appToaster } from "~/lib/good-toaster.tsx"
import { createSession } from "./actions.ts"
import styles from "./dialog.style.ts"
import { SessionLaunchFormState } from "./launch-form-state.ts"
import { SessionLaunchForm } from "./launch-form.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"

export default function SessionLaunchDialog(props: { dialog: UseDialogReturn }) {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const form = useModel(SessionLaunchFormState)
  const hasAdapterSelector =
    (form.adapterCatalog.value?.adapters.length ?? 0) > 0 && props.dialog.open
  const hasBranchSelector =
    form.draftLocation.value === "worktree" &&
    (form.launchPreview.value?.branches.length ?? 0) > 0 &&
    props.dialog.open
  const hasLocationSelector = form.launchPreview.value !== null && props.dialog.open
  const hasModelSelector =
    (form.launchModelConfig.value.models?.availableModels.length ?? 0) > 0 && props.dialog.open
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
      const sessionTitle = getSessionDisplayTitle(session)

      form.reset(projectPath)
      workbenchTabSet.openOrFocusTab({
        id: `session:${session.id}`,
        kind: "sessionChat",
        title: sessionTitle,
        payload: {
          projectPath,
          sessionId: session.id,
        },
        dirty: false,
      })
      // Defer the toast until the submit control finishes clearing its editor after onSubmit.
      window.setTimeout(() => {
        appToaster.create({
          description: sessionTitle,
          duration: 2800,
          title: "Session launched",
          type: "success",
        })
      }, 0)
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
    commandContext.sessionInputHasAdapterSelector.value = hasAdapterSelector
    commandContext.sessionInputHasBranchSelector.value = hasBranchSelector
    commandContext.sessionInputHasLocationSelector.value = hasLocationSelector
    commandContext.sessionInputCanSubmit.value = canSubmit
    commandContext.sessionInputHasModelSelector.value = hasModelSelector
    commandContext.sessionInputHasProjectSelector.value = hasProjectSelector
    commandContext.sessionInputHasThinkingLevel.value = hasThinkingLevel

    return () => {
      commandContext.sessionInputActive.value = false
      commandContext.sessionInputHasAdapterSelector.value = false
      commandContext.sessionInputHasBranchSelector.value = false
      commandContext.sessionInputHasLocationSelector.value = false
      commandContext.sessionInputCanSubmit.value = false
      commandContext.sessionInputHasModelSelector.value = false
      commandContext.sessionInputHasProjectSelector.value = false
      commandContext.sessionInputHasThinkingLevel.value = false
    }
  }, [
    canSubmit,
    hasAdapterSelector,
    hasBranchSelector,
    hasLocationSelector,
    hasModelSelector,
    hasProjectSelector,
    hasThinkingLevel,
    props.dialog.open,
  ])

  useAppCommand(AppCommand.sessionInput.openProjectSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("project")
  })

  useAppCommand(AppCommand.sessionInput.openAdapterSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("adapter")
  })

  useAppCommand(AppCommand.sessionInput.openLocationSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("location")
  })

  useAppCommand(AppCommand.sessionInput.openBranchSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("branch")
  })

  useAppCommand(AppCommand.sessionInput.openModelSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("model")
  })

  useAppCommand(AppCommand.sessionInput.openThinkingLevelSelector, () => {
    if (!props.dialog.open) {
      return
    }

    form.setOpenPicker("thinking")
  })

  useAppCommand(AppCommand.sessionInput.submit, () => {
    if (!props.dialog.open) {
      return
    }

    void launchSession()
  })

  return (
    <DialogPortal>
      <Dialog.Backdrop class={styles.backdrop} />
      <Dialog.Positioner class={styles.positioner}>
        <Dialog.Content class={styles.content}>
          <Dialog.Title class={styles.title}>Launch session</Dialog.Title>
          <Dialog.CloseTrigger asChild>
            <button class={styles.closeButton} type="button">
              <X size={16} strokeWidth={2.2} />
            </button>
          </Dialog.CloseTrigger>
          <SessionLaunchForm
            form={form}
            onEscape={() => {
              // Lexical binds Escape to blur by default, so close the dialog explicitly here.
              props.dialog.setOpen(false)
            }}
            onSubmit={launchSession}
            projects={projectRegistry.projectList}
          />
        </Dialog.Content>
      </Dialog.Positioner>
    </DialogPortal>
  )
}
