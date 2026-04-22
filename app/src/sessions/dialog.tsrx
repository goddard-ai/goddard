import { Dialog, type UseDialogReturn } from "@ark-ui/react/dialog"
import { useModel } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { commandContext } from "~/commands/command-context.ts"
import { DialogPortal } from "~/lib/dialog-portal.tsrx"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { createSession } from "./actions.ts"
import styles from "./dialog.style.ts"
import { SessionLaunchFormState } from "./launch-form-state.ts"
import { SessionLaunchForm } from "./launch-form.tsrx"
import { getSessionDisplayTitle } from "./presentation.ts"

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
  const hasAdapterSelector = (form.adapterCatalog.value?.adapters.length ?? 0) > 0 && isDialogOpen
  const hasBranchSelector =
    form.draftLocation.value === "worktree" &&
      (form.launchPreview.value?.branches.length ?? 0) > 0 &&
      isDialogOpen
  const hasLocationSelector = form.launchPreview.value !== null && isDialogOpen
  const hasModelSelector =
    (form.launchModelConfig.value.models?.availableModels.length ?? 0) > 0 && isDialogOpen
  const hasProjectSelector = projects.length > 0 && isDialogOpen
  const hasThinkingLevel = form.thinkingOption.value !== null && isDialogOpen
  const canSubmit = form.canSubmit.value && isDialogOpen

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
      workbenchTabSet.openOrFocusTab(
        {
          id: `session:${session.id}`,
          kind: "sessionChat",
          title: sessionTitle,
          payload: {
            projectPath,
            sessionId: session.id,
          },
          dirty: false,
        },
      )
      // Defer the toast until the submit control finishes clearing its editor after onSubmit.
      window.setTimeout(() => {
        appToaster.create(
          {
            description: sessionTitle,
            duration: 2800,
            title: "Session launched",
            type: "success",
          },
        )
      }, 0)
    } catch (error) {
      console.error("Failed to create session.", error)
    }
  }

  function openProjectSelector() {
    if (!isDialogOpen) {
      return
    }

    form.setOpenPicker("project")
  }

  function openAdapterSelector() {
    if (!isDialogOpen) {
      return
    }

    form.setOpenPicker("adapter")
  }

  function openLocationSelector() {
    if (!isDialogOpen) {
      return
    }

    form.setOpenPicker("location")
  }

  function openBranchSelector() {
    if (!isDialogOpen) {
      return
    }

    form.setOpenPicker("branch")
  }

  function openModelSelector() {
    if (!isDialogOpen) {
      return
    }

    form.setOpenPicker("model")
  }

  function openThinkingLevelSelector() {
    if (!isDialogOpen) {
      return
    }

    form.setOpenPicker("thinking")
  }

  function submitSessionLaunch() {
    if (!isDialogOpen) {
      return
    }

    void launchSession()
  }

  useEffect(() => {
    if (isDialogOpen) {
      form.reset(activeProjectPath)
    }
  }, [activeProjectPath, form, isDialogOpen])

  useEffect(() => {
    commandContext.sessionInputActive.value = isDialogOpen
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
    isDialogOpen,
  ])

  useAppCommand(AppCommand.sessionInput.openProjectSelector, openProjectSelector)
  useAppCommand(AppCommand.sessionInput.openAdapterSelector, openAdapterSelector)
  useAppCommand(AppCommand.sessionInput.openLocationSelector, openLocationSelector)
  useAppCommand(AppCommand.sessionInput.openBranchSelector, openBranchSelector)
  useAppCommand(AppCommand.sessionInput.openModelSelector, openModelSelector)
  useAppCommand(AppCommand.sessionInput.openThinkingLevelSelector, openThinkingLevelSelector)
  useAppCommand(AppCommand.sessionInput.submit, submitSessionLaunch)

  <tsx:react>
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
          <SessionLaunchForm form={form} onEscape={() => {
            props.dialog.setOpen(false)
          }} onSubmit={launchSession} projects={projects} />
        </Dialog.Content>
      </Dialog.Positioner>
    </DialogPortal>
  </tsx:react>
}

export default SessionLaunchDialog
