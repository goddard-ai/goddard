import { Dialog, useDialog, type UseDialogProps, type UseDialogReturn } from "@ark-ui/react/dialog"
import { Suspense, lazy } from "preact/compat"
import { useMemo } from "preact/hooks"

import { useAppCommand, type AppCommand } from "./app-command.ts"

type CommandDialogContentModule = {
  default: preact.FunctionComponent<{ dialog: UseDialogReturn }>
}

/** Props for one command-driven dialog wrapper. */
export type CommandDialogProps = {
  command: AppCommand
  content: () => Promise<CommandDialogContentModule>
  dialogProps?: UseDialogProps
}

/** Provides one Ark dialog controller for a lazily loaded command-dialog surface. */
export function CommandDialog(props: CommandDialogProps) {
  const Content = useMemo(() => lazy(props.content), [])

  const dialog = useDialog(props.dialogProps)
  useAppCommand(props.command, () => {
    dialog.setOpen(true)
  })

  return (
    <Dialog.RootProvider value={dialog}>
      <Suspense fallback={null}>
        <Content dialog={dialog} />
      </Suspense>
    </Dialog.RootProvider>
  )
}
