import { Dialog, useDialog, type UseDialogProps, type UseDialogReturn } from "@ark-ui/react/dialog"
import { useListener } from "preact-sigma"
import { Suspense, lazy } from "preact/compat"
import { useId, useMemo } from "preact/hooks"
import { concat } from "radashi"

import { useModalStackEntry } from "~/lib/modal-stack.ts"
import { menuPortalId } from "~/lib/menu-portal.tsrx"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { AppCommand, useAppCommand } from "./app-command.ts"
import type { AppCommand as AppCommandType } from "./app-command.ts"
import { CommandLayerProvider } from "./command-layer.tsrx"

type CommandDialogContentModule = {
  default: preact.FunctionComponent<{ dialog: UseDialogReturn }>;
}

/** Props for one command-driven dialog wrapper. */
export type CommandDialogProps = {
  command: AppCommandType;
  content: () => Promise<CommandDialogContentModule>;
  dialogProps?: UseDialogProps;
}

/** Provides one Ark dialog controller for a lazily loaded command-dialog surface. */
export component CommandDialog(props: CommandDialogProps) {
  const Content = useMemo(() => lazy(props.content), [])
  const dialogId = useId()

  const dialog = useDialog(
    {
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
    },
  )

  useModalStackEntry(
    {
      id: `command-dialog:${dialogId}`,
      open: dialog.open,
      close() {
        dialog.setOpen(false)
      },
    },
  )

  useListener(globalEventHub, "commandDialogActivated", ({ dialogId: activeDialogId }) => {
    if (activeDialogId !== dialogId) {
      dialog.setOpen(false)
    }
  })

  useAppCommand(props.command, () => {
    dialog.setOpen(true)
  })

  useAppCommand(AppCommand.workbench.closeActiveTab, () => {
    dialog.setOpen(false)
  }, {
    active: dialog.open,
    layerId: dialogId,
  })

  <CommandLayerProvider active={dialog.open} id={dialogId}>
    <Dialog.RootProvider value={dialog} lazyMount unmountOnExit>
      <Suspense fallback={null}>
        <Content {dialog} />
      </Suspense>
    </Dialog.RootProvider>
  </CommandLayerProvider>
}
