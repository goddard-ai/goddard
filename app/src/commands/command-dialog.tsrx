import { Dialog, useDialog, type UseDialogProps, type UseDialogReturn } from "@ark-ui/react/dialog"
import { css } from "@goddard-ai/styled-system/css"
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

const dialogFailure = {
  backdrop: css({
    position: "fixed",
    inset: "0",
    backgroundColor: "rgba(7, 10, 15, 0.42)",
  }),
  positioner: css({
    position: "fixed",
    inset: "0",
    display: "grid",
    placeItems: "start center",
    paddingTop: "18vh",
  }),
  content: css({
    display: "grid",
    gap: "14px",
    width: "min(420px, calc(100vw - 32px))",
    padding: "18px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "surface",
    boxShadow: "0 18px 60px rgba(0, 0, 0, 0.28)",
    color: "text",
  }),
  copy: css({
    display: "grid",
    gap: "7px",
  }),
  title: css({
    fontSize: "1rem",
    fontWeight: "720",
    lineHeight: "1.35",
  }),
  description: css({
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.55",
  }),
  actions: css({
    display: "flex",
    justifyContent: "flex-end",
    gap: "8px",
  }),
  button: css({
    height: "30px",
    paddingInline: "12px",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "panel",
    color: "text",
    cursor: "pointer",
    fontSize: "0.86rem",
    fontWeight: "640",
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
    _hover: {
      backgroundColor: "background",
    },
  }),
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
    <Dialog.Backdrop class={dialogFailure.backdrop} />
    <Dialog.Positioner class={dialogFailure.positioner}>
      <Dialog.Content class={dialogFailure.content}>
        <div class={dialogFailure.copy}>
          <Dialog.Title class={dialogFailure.title}>"Dialog unavailable"</Dialog.Title>
          <Dialog.Description class={dialogFailure.description}>
            "This dialog failed to render. The rest of the app is still available."
          </Dialog.Description>
        </div>
        <div class={dialogFailure.actions}>
          <button
            class={dialogFailure.button}
            type="button"
            onClick={() => {
              props.dialog.setOpen(false)
            }}
          >
            "Close"
          </button>
          <button class={dialogFailure.button} type="button" onClick={props.retry}>
            "Try again"
          </button>
        </div>
      </Dialog.Content>
    </Dialog.Positioner>
  </DialogPortal>
}
