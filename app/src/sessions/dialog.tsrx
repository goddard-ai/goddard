import { Dialog, type UseDialogReturn } from "@ark-ui/react/dialog"
import { useModel } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { DialogPortal } from "~/lib/dialog-portal.tsrx"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { createSession } from "./mutations.ts"
import styles from "./dialog.style.ts"
import { SessionLaunchFormState } from "./launch-form-state.ts"
import { SessionLaunchForm } from "./launch-form.tsrx"
import { getSessionDisplayTitle } from "./display.ts"

component SessionLaunchDialog(props: {
  dialog: UseDialogReturn;
}) {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const form = useModel(SessionLaunchFormState)
  const activeProjectPath = projectContext.activeProjectPath
  const isDialogOpen = props.dialog.open
  const projects = projectRegistry.projectList
  const hasAdapterSelector = (form.adapterCatalog.value?.adapters.length ?? 0) > 0
  const hasBranchSelector =
    form.draftLocation.value === "worktree" && (form.launchPreview.value?.branches.length ?? 0) > 0
  const hasLocationSelector = form.launchPreview.value !== null
  const hasModelSelector = (form.launchModelConfig.value.models?.availableModels.length ?? 0) > 0
  const hasProjectSelector = projects.length > 0
  const hasThinkingLevel = form.thinkingOption.value !== null
  const canSubmit = form.canSubmit.value

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
        kind: "sessionChat",
        payload: {
          projectPath,
          sessionId: session.id,
          sessionTitle,
        },
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
    if (isDialogOpen && form.draftProjectPath.value === null) {
      form.reset(activeProjectPath)
    }
  }, [activeProjectPath, form, form.draftProjectPath.value, isDialogOpen])

  useAppCommand(AppCommand.sessionInput.openProjectSelector, () => {
    form.setOpenPicker("project")
  }, { active: hasProjectSelector })
  useAppCommand(AppCommand.sessionInput.openAdapterSelector, () => {
    form.setOpenPicker("adapter")
  }, { active: hasAdapterSelector })
  useAppCommand(AppCommand.sessionInput.openLocationSelector, () => {
    form.setOpenPicker("location")
  }, { active: hasLocationSelector })
  useAppCommand(AppCommand.sessionInput.openBranchSelector, () => {
    form.setOpenPicker("branch")
  }, { active: hasBranchSelector })
  useAppCommand(AppCommand.sessionInput.openModelSelector, () => {
    form.setOpenPicker("model")
  }, { active: hasModelSelector })
  useAppCommand(AppCommand.sessionInput.openThinkingLevelSelector, () => {
    form.setOpenPicker("thinking")
  }, { active: hasThinkingLevel })
  useAppCommand(AppCommand.sessionInput.submit, () => {
    void launchSession()
  }, { active: canSubmit })

  <DialogPortal>
    <Dialog.Backdrop class={styles.backdrop} />
    <Dialog.Positioner class={styles.positioner}>
      <Dialog.Content class={styles.content}>
        <Dialog.Title class={styles.title}>"Launch session"</Dialog.Title>
        <Dialog.CloseTrigger asChild>
          <button class={styles.closeButton} type="button">
            <X size={16} strokeWidth={2.2} />
          </button>
        </Dialog.CloseTrigger>
        <SessionLaunchForm
          {form}
          onEscape={() => {
            props.dialog.setOpen(false)
          }}
          onSubmit={launchSession}
          {projects}
        />
      </Dialog.Content>
    </Dialog.Positioner>
  </DialogPortal>
}

export default SessionLaunchDialog
