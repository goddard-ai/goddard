import { Dialog, useDialog, type UseDialogProps, type UseDialogReturn } from "@ark-ui/react/dialog"
import { useListener } from "preact-sigma"
import { lazy } from "preact/compat"
import { useId, useMemo } from "preact/hooks"
import { concat } from "radashi"

import { DialogPortal } from "~/lib/dialog-portal.tsrx"
import { useModalStackEntry } from "~/lib/modal-stack.ts"
import { menuPortalId } from "~/lib/menu-portal.tsrx"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppCommand, useAppCommand } from "./app-command.ts"
import type { AppCommand as AppCommandType } from "./app-command.ts"
import { CommandLayerProvider } from "./command-layer.tsrx"
import styles from "./command-dialog.style.ts"

/** Lazy command-dialog modules expose one dialog-controlled content component. */
interface CommandDialogContentModule {
  default: preact.FunctionComponent<{ dialog: UseDialogReturn }>
}

/** Props for one command-driven dialog wrapper. */
export interface CommandDialogProps {
  command: AppCommandType
  content: () => Promise<CommandDialogContentModule>
  dialogProps?: UseDialogProps
}

/** Provides one Ark dialog controller for a lazily loaded command-dialog surface. */
export component CommandDialog(props: CommandDialogProps) {
  const Content = useMemo(() => lazy(props.content), [])
  const dialogId = useId()

  const dialog = useDialog({
    ...props.dialogProps,
    onOpenChange: (details) => {
      props.dialogProps?.onOpenChange?.(details)

      if (details.open) {
        globalEventHub.emit("commandDialogActivated", { dialogId })
      }
    },
    persistentElements: concat(
      () => document.getElementById(menuPortalId),
      props.dialogProps?.persistentElements,
    ),
  })

  useModalStackEntry({
    id: `command-dialog:${dialogId}`,
    open: dialog.open,
    close() {
      dialog.setOpen(false)
    },
  })

  useListener(globalEventHub, "commandDialogActivated", ({ dialogId: activeDialogId }) => {
    if (activeDialogId !== dialogId) {
      dialog.setOpen(false)
    }
  })

  useAppCommand(props.command, () => {
    dialog.setOpen(true)
  })

  <CommandLayerProvider active={dialog.open}>
    useAppCommand(AppCommand.workbench.closeActiveTab, () => {
      dialog.setOpen(false)
    })

    <Dialog.RootProvider value={dialog} lazyMount unmountOnExit>
      try {
        <Content {dialog} />
      } pending {
        <></>
      } catch (error, retry) {
        console.error("Command dialog render failed.", error)
        <CommandDialogFailure {dialog} {retry} />
      }
    </Dialog.RootProvider>
  </CommandLayerProvider>
}

component CommandDialogFailure(props: {
  dialog: UseDialogReturn;
  retry: () => void;
}) {
  <DialogPortal>
    <Dialog.Backdrop class={styles.failureBackdrop} />
    <Dialog.Positioner class={styles.failurePositioner}>
      <Dialog.Content class={styles.failureContent}>
        <div class={styles.failureCopy}>
          <Dialog.Title class={styles.failureTitle}>"Dialog unavailable"</Dialog.Title>
          <Dialog.Description class={styles.failureDescription}>
            "This dialog failed to render. The rest of the app is still available."
          </Dialog.Description>
        </div>
        <div class={styles.failureActions}>
          <button
            class={styles.failureButton}
            type="button"
            onClick={() => {
              props.dialog.setOpen(false)
            }}
          >
            "Close"
          </button>
          <button class={styles.failureButton} type="button" onClick={props.retry}>
            "Try again"
          </button>
        </div>
      </Dialog.Content>
    </Dialog.Positioner>
  </DialogPortal>
}
