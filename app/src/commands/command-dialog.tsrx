import { t } from "@lingui/core/macro"
import { type Signal, useSignal } from "@preact/signals"
import { useListener } from "preact-sigma"
import { lazy } from "preact/compat"
import { useId, useMemo } from "preact/hooks"

import { Modal } from "@goddard-ai/ui-primitives"
import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { useAppCommand } from "./app-command.ts"
import type { AppCommand as AppCommandType } from "./app-command.ts"
import { CommandLayerProvider } from "./command-layer.tsrx"
import styles from "./command-dialog.style.ts"

/** Lazy command-dialog modules expose one dialog-controlled content component. */
interface CommandDialogContentModule {
  default: preact.FunctionComponent<{ open: Signal<boolean> }>
}

/** Props for one command-driven dialog wrapper. */
export interface CommandDialogProps {
  command: AppCommandType
  content: () => Promise<CommandDialogContentModule>
}

/** Provides one signal-backed modal controller for a lazily loaded command-dialog surface. */
export function CommandDialog(props: CommandDialogProps) @{
  const Content = useMemo(() => lazy(props.content), [])
  const dialogId = useId()
  const open = useSignal(false)

  function openDialog() {
    open.value = true
    globalEventHub.emit("commandDialogActivated", { dialogId })
  }

  useListener(globalEventHub, "commandDialogActivated", ({ dialogId: activeDialogId }) => {
    if (activeDialogId !== dialogId) {
      open.value = false
    }
  })

  useAppCommand(props.command, openDialog)

  <CommandLayerProvider active={open.value}>
    @if (open.value) {
      @try {
        <Content {open} />
      } @pending {
      } @catch (error, retry) {
        useErrorBoundaryReset(retry)
        console.error("Command dialog render failed.", error)

        <CommandDialogFailure {open} {retry} />
      }
    }
  </CommandLayerProvider>
}

function CommandDialogFailure(props: {
  open: Signal<boolean>;
  retry: () => void;
}) @{
  const titleId = useId()
  const descriptionId = useId()

  <Modal
    open={props.open}
    {titleId}
    {descriptionId}
    backdropClass={styles.failureBackdrop}
    positionerClass={styles.failurePositioner}
    contentClass={styles.failureContent}
  >
    <div class={styles.failureCopy}>
      <h2 id={titleId} class={styles.failureTitle}>
        {t`Dialog unavailable`}
      </h2>
      <p id={descriptionId} class={styles.failureDescription}>
        {t`This dialog failed to render. The rest of the app is still available.`}
      </p>
    </div>
    <div class={styles.failureActions}>
      <button
        class={styles.failureButton}
        type="button"
        onClick={() => {
          props.open.value = false
        }}
      >
        {t`Close`}
      </button>
      <button class={styles.failureButton} type="button" onClick={props.retry}>
        {t`Try again`}
      </button>
    </div>
  </Modal>
}
