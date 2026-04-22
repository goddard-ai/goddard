import { Dialog, useDialog, type UseDialogProps, type UseDialogReturn } from "@ark-ui/react/dialog"
import { useListener } from "preact-sigma"
import { Suspense, lazy } from "preact/compat"
import { useId, useMemo } from "preact/hooks"
import { concat } from "radashi"

import { menuPortalId } from "~/lib/menu-portal.tsrx"
import { globalEventHub } from "~/shared/global-event-hub.ts"
import { useAppCommand, type AppCommand } from "./app-command.ts"

type CommandDialogContentModule = {
  default: preact.FunctionComponent<{ dialog: UseDialogReturn }>;
}

/** Props for one command-driven dialog wrapper. */
export type CommandDialogProps = {
  command: AppCommand;
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

  useListener(globalEventHub, "commandDialogActivated", ({ dialogId: activeDialogId }) => {
    if (activeDialogId !== dialogId) {
      dialog.setOpen(false)
    }
  })

  useAppCommand(props.command, () => {
    dialog.setOpen(true)
  })

  <Dialog.RootProvider value={dialog} lazyMount unmountOnExit>
    <Suspense fallback={null}>
      <Content {dialog} />
    </Suspense>
  </Dialog.RootProvider>
}
