import { t } from "@lingui/core/macro"
import { type Signal } from "@preact/signals"
import { useModel, useSignal } from "@preact/signals"
import { X } from "lucide-react"
import { useEffect, useId } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { Modal } from "@goddard-ai/ui-primitives"
import { goddardSdk } from "~/sdk.ts"
import { recordSessionLaunchAgentUse } from "./launch-preferences.ts"
import styles from "./dialog.style.ts"
import { SessionLaunchFormState } from "./launch-form-state.ts"
import { isKeptSessionLaunchLease } from "./launch-lease-keeper.tsrx"
import {
  createSession,
  releaseSessionLaunchLease,
  releaseSessionLaunchWorktree,
} from "./mutations.ts"
import { SessionLaunchForm } from "./launch-form.tsrx"
import { getSessionDisplayTitle } from "./display.ts"

export default function SessionLaunchDialog(props: {
  open: Signal<boolean>;
}) @{
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const form = useModel(SessionLaunchFormState)
  const isLaunching = useSignal(false)
  const activeProjectPath = projectContext.activeProjectPath
  const isDialogOpen = props.open.value
  const titleId = useId()
  const projects = projectRegistry.projectList
  const initialProjectPath = activeProjectPath ?? projects[0]?.path ?? null
  const hasAdapterSelector = (form.adapterCatalog.value?.adapters.length ?? 0) > 0
  const hasBranchSelector =
    (form.launchPreview.value?.branches.length ?? 0) > 0 &&
      !(form.draftLocation.value === "local" && form.launchPreview.value?.dirty === true) &&
      !(form.draftLocation.value === "local" && form.launchPreview.value?.bare === true)
  const hasLocationSelector = !!form.launchPreview.value?.repoRoot
  const hasModelSelector = (form.launchModelConfig.value.models?.availableModels.length ?? 0) > 0
  const hasProjectSelector = projects.length > 0
  const hasSubpackageSelector = form.draftProjectPath.value !== null
  const hasThinkingLevel = form.thinkingOption.value !== null
  const canSubmit = form.canSubmit.value && !isLaunching.value

  async function launchSession() {
    if (isLaunching.value) {
      return
    }

    const sessionInput = form.sessionInput.value

    if (!sessionInput) {
      return
    }

    isLaunching.value = true
    const toastId = appToaster.create({
      description: form.effectiveCwd.value,
      title: t`Launching session`,
      type: "loading",
    })

    try {
      const projectPath = form.draftProjectPath.value
      const launchLeaseId = form.launchPreview.value?.launchLeaseId
      recordSessionLaunchAgentUse(form.draftAdapterId.value)
      const { session } = await createSession(sessionInput)
      const sessionTitle = getSessionDisplayTitle(session)

      if (!isKeptSessionLaunchLease(launchLeaseId)) {
        void releaseSessionLaunchLease(launchLeaseId).catch((error) => {
          console.error("Failed to release launch lease.", error)
        })
      }
      form.reset(projectPath)
      workbenchTabSet.openOrFocusTab({
        kind: "sessionChat",
        props: {
          relatedFilesystemPath: projectPath,
          sessionId: session.id,
          sessionTitle,
        },
      })
      appToaster.update(toastId, {
        description: sessionTitle,
        duration: 2800,
        title: t`Session launched`,
        type: "success",
      })
    } catch (error) {
      console.error("Failed to create session.", error)
      appToaster.update(toastId, {
        description: t`The session could not be created.`,
        title: t`Session launch failed`,
        type: "error",
      })
    } finally {
      isLaunching.value = false
    }
  }

  useEffect(() => {
    if (isDialogOpen && form.draftProjectPath.value === null && initialProjectPath !== null) {
      form.reset(initialProjectPath)
    }
  }, [form, form.draftProjectPath.value, initialProjectPath, isDialogOpen])

  useEffect(() => {
    const cwd = form.effectiveCwd.value

    if (!isDialogOpen || !cwd || !form.launchPreview.value?.repoRoot) {
      form.launchWorktreeId.value = null
      return
    }

    const baseBranchName = form.draftBaseBranchName.value ?? undefined
    let active = true
    let launchWorktreeId: string | null = null

    void goddardSdk.session.launchWorktree.prepare({
      cwd,
      baseBranchName,
    }).then((response) => {
      launchWorktreeId = response.launchWorktreeId

      if (!active) {
        void releaseSessionLaunchWorktree(launchWorktreeId).catch((error) => {
          console.error("Failed to release launch worktree.", error)
        })
        return
      }

      form.launchWorktreeId.value = launchWorktreeId
    }).catch((error) => {
      console.error("Failed to prepare launch worktree.", error)
    })

    return () => {
      active = false

      if (form.launchWorktreeId.value === launchWorktreeId) {
        form.launchWorktreeId.value = null
      }

      void releaseSessionLaunchWorktree(launchWorktreeId).catch((error) => {
        console.error("Failed to release launch worktree.", error)
      })
    }
  }, [
    form,
    form.draftBaseBranchName.value,
    form.effectiveCwd.value,
    form.launchPreview.value?.repoRoot,
    isDialogOpen,
  ])

  useAppCommand(AppCommand.sessionInput.openProjectSelector, () => {
    form.setOpenPicker("project")
  }, { active: hasProjectSelector && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openSubpackageSelector, () => {
    form.setOpenPicker("subpackage")
  }, { active: hasSubpackageSelector && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openAdapterSelector, () => {
    form.setOpenPicker("adapter")
  }, { active: hasAdapterSelector && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openLocationSelector, () => {
    form.cycleLaunchLocation()
  }, { active: hasLocationSelector && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openBranchSelector, () => {
    form.setOpenPicker("branch")
  }, { active: hasBranchSelector && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openApprovalPresetSelector, () => {
    form.setOpenPicker("mode")
  }, { active: form.modeOption.value !== null && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openModelSelector, () => {
    form.setOpenPicker("model")
  }, { active: hasModelSelector && !isLaunching.value })
  useAppCommand(AppCommand.sessionInput.openThinkingLevelSelector, () => {
    form.setOpenPicker("thinking")
  }, { active: hasThinkingLevel && !isLaunching.value })
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
    <header class={styles.header}>
      <h2 id={titleId} class={styles.title}>
        {t`Launch session`}
      </h2>
      <p class={styles.description}>
        {t`Choose the project, agent harness, launch options, and initial prompt.`}
      </p>
    </header>
    <button
      aria-label={t`Close launch session dialog`}
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
      disabled={isLaunching.value}
      onEscape={() => {
        props.open.value = false
      }}
      onSubmit={launchSession}
      {projects}
    />
  </Modal>
}
