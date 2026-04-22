/** Renders the app-wide command palette. */
import { Dialog, UseDialogReturn } from "@ark-ui/react/dialog"
import { token } from "@goddard-ai/styled-system/tokens"
import { Command } from "ark-cmdk"
import { Search } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useShortcutRegistry } from "./app-state-context.tsrx"
import styles from "./command-palette.style.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandAvailable } from "./commands/command-context.ts"
import { DialogPortal } from "./lib/dialog-portal.tsrx"

component CommandPalette(props: { dialog: UseDialogReturn }) {
  const { open } = props.dialog
  const shortcutRegistry = useShortcutRegistry()

  const [search, setSearch] = useState("")
  useEffect(() => {
    if (!open && search.length > 0) {
      setSearch("")
    }
  }, [open, search])

  const visibleCommands = appCommandList.filter(
    (command) => command.id !== "navigation.openCommandPalette" &&
      isCommandAvailable(shortcutRegistry.runtime, command),
  )

  <DialogPortal>
    <Dialog.Backdrop class={styles.backdrop} />
    <Dialog.Positioner class={styles.positioner}>
      <Dialog.Content class={styles.content}>
        <Dialog.Title class={styles.title}>{"Command menu"}</Dialog.Title>
        <Command.Root
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
          {search}
        >
          <div class={styles.searchRow}>
            <Search aria-hidden={true} class={styles.searchIcon} size={16} />
            <Command.Input autoFocus={true} class={styles.input} placeholder="Type a command" />
          </div>
          <Command.List
            class={styles.list}
            empty={<tsx:react>
              <div class={styles.empty}>No matching commands.</div>
            </tsx:react>}
          >
            {(item, state) => {
              const Icon = item.icon
              const shortcutBinding = shortcutRegistry.resolvedBindings[item.id]?.[0]
              const shortcut =
                typeof shortcutBinding === "string"
                  ? shortcutBinding
                  : shortcutBinding?.combo ?? shortcutBinding?.sequence

              return <tsx:react>
                <div class={styles.item} data-active={state.active ? "true" : "false"} style={{
                  gridTemplateColumns: Icon ? "16px minmax(0, 1fr) auto" : "minmax(0, 1fr) auto",
                }}>
                  {Icon
                    ? <tsx:react>
                        <Icon aria-hidden={true} size={16} strokeWidth={2} style={{
                          color: state.active
                            ? token.var("colors.text")
                            : token.var("colors.muted"),
                        }} />
                      </tsx:react>
                    : null}
                  <span class={styles.itemLabel}>
                    {item.label}
                  </span>
                  {shortcut
                    ? <tsx:react>
                        <span class={styles.shortcut}>
                          {shortcut}
                        </span>
                      </tsx:react>
                    : null}
                </div>
              </tsx:react>
            }}
          </Command.List>
        </Command.Root>
      </Dialog.Content>
    </Dialog.Positioner>
  </DialogPortal>
}

export default CommandPalette
