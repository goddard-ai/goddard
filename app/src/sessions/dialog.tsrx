import { type Signal } from "@preact/signals"
import { useModel } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect, useId } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { Modal } from "~/lib/modal.tsrx"
import { createSession } from "./mutations.ts"
import styles from "./dialog.style.ts"
import { SessionLaunchFormState } from "./launch-form-state.ts"
import { SessionLaunchForm } from "./launch-form.tsrx"
import { getSessionDisplayTitle } from "./display.ts"

component SessionLaunchDialog(props: {
  open: Signal<boolean>;
}) {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const form = useModel(SessionLaunchFormState)
  const activeProjectPath = projectContext.activeProjectPath
  const isDialogOpen = props.open.value
  const titleId = useId()
  const projects = projectRegistry.projectList
  const initialProjectPath = activeProjectPath ?? projects[0]?.path ?? null
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
    if (isDialogOpen && form.draftProjectPath.value === null && initialProjectPath !== null) {
      form.reset(initialProjectPath)
    }
  }, [form, form.draftProjectPath.value, initialProjectPath, isDialogOpen])

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

  <Modal
    open={props.open}
    {titleId}
    backdropClass={styles.backdrop}
    positionerClass={styles.positioner}
    contentClass={styles.content}
  >
    <h2 id={titleId} class={styles.title}>"Launch session"</h2>
    <button
      class={styles.closeButton}
      type="button"
      onClick={() => {
        props.open.value = false
      }}
    >
      <X size={16} strokeWidth={2.2} />
    </button>
    <SessionLaunchForm
      {form}
      onEscape={() => {
        props.open.value = false
      }}
      onSubmit={launchSession}
      {projects}
    />
  </Modal>
}

export default SessionLaunchDialog
