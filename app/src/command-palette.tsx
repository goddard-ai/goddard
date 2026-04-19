/** Renders the app-wide command palette. */
import { Dialog, UseDialogReturn } from "@ark-ui/react/dialog"
import { token } from "@goddard-ai/styled-system/tokens"
import { Command } from "ark-cmdk"
import { Search } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useShortcutRegistry } from "./app-state-context.tsx"
import styles from "./command-palette.style.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandAvailable } from "./commands/command-context.ts"
import { DialogPortal } from "./lib/dialog-portal.tsx"

export default function CommandPalette(props: { dialog: UseDialogReturn }) {
  const { open } = props.dialog
  const shortcutRegistry = useShortcutRegistry()

  const [search, setSearch] = useState("")
  useEffect(() => {
    if (!open && search.length > 0) {
      setSearch("")
    }
  }, [open, search])

  const visibleCommands = appCommandList.filter((command) =>
    isCommandAvailable(shortcutRegistry.runtime, command),
  )

  return (
    <DialogPortal>
      <Dialog.Backdrop class={styles.backdrop} />
      <Dialog.Positioner class={styles.positioner}>
        <Dialog.Content class={styles.content}>
          <Dialog.Title class={styles.title}>Command menu</Dialog.Title>
          <Command.Root<AppCommand>
            class={styles.root}
            itemToKeywords={(command) => command.keywords}
            itemToString={(command) => command.label}
            itemToValue={(command) => command.id}
            items={visibleCommands}
            label="Command menu"
            onSearchChange={setSearch}
            onSelect={(command) => {
              props.dialog.setOpen(false)
              command()
            }}
            search={search}
          >
            <div class={styles.searchRow}>
              <Search aria-hidden={true} class={styles.searchIcon} size={16} />
              <Command.Input autoFocus={true} class={styles.input} placeholder="Type a command" />
            </div>
            <Command.List<AppCommand>
              class={styles.list}
              empty={<div class={styles.empty}>No matching commands.</div>}
            >
              {(item, state) => {
                const Icon = item.icon
                const shortcutBinding = shortcutRegistry.resolvedBindings[item.id]?.[0]
                const shortcut =
                  typeof shortcutBinding === "string"
                    ? shortcutBinding
                    : (shortcutBinding?.combo ?? shortcutBinding?.sequence)

                return (
                  <div
                    class={styles.item}
                    data-active={state.active ? "true" : "false"}
                    style={{
                      gridTemplateColumns: Icon
                        ? "16px minmax(0, 1fr) auto"
                        : "minmax(0, 1fr) auto",
                    }}
                  >
                    {Icon ? (
                      <Icon
                        aria-hidden={true}
                        size={16}
                        strokeWidth={2}
                        style={{
                          color: state.active
                            ? token.var("colors.text")
                            : token.var("colors.muted"),
                        }}
                      />
                    ) : null}
                    <span class={styles.itemLabel}>{item.label}</span>
                    {shortcut ? <span class={styles.shortcut}>{shortcut}</span> : null}
                  </div>
                )
              }}
            </Command.List>
          </Command.Root>
        </Dialog.Content>
      </Dialog.Positioner>
    </DialogPortal>
  )
}
