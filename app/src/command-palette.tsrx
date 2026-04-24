/** Renders the app-wide command palette. */
import { Dialog, UseDialogReturn } from "@ark-ui/react/dialog"
import { token } from "@goddard-ai/styled-system/tokens"
import { Command } from "ark-cmdk"
import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { useShortcutRegistry } from "./app-state-context.tsrx"
import styles from "./command-palette.style.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandAvailable } from "./commands/command-context.ts"
import { DialogPortal } from "./lib/dialog-portal.tsrx"
import { isEmptyQuery } from "./lib/search-query.ts"

const preparedAppCommands = appCommandList.map(
  (command) => ({
    command,
    preparedKeywords: command.keywords ? fuzzysort.prepare(command.keywords.join("\n")) : null,
    preparedLabel: fuzzysort.prepare(command.label),
  }),
)

component CommandPalette(props: { dialog: UseDialogReturn }) {
  const { open } = props.dialog
  const shortcutRegistry = useShortcutRegistry()

  const [search, setSearch] = useState("")
  useEffect(() => {
    if (!open && search.length > 0) {
      setSearch("")
    }
  }, [open, search])

  const visibleCommands = (() => {
    const availableCommands = preparedAppCommands.filter(
      (entry) => entry.command.id !== "navigation.openCommandPalette" &&
        isCommandAvailable(shortcutRegistry.runtime, entry.command),
    )

    if (isEmptyQuery(search)) {
      return availableCommands.map((entry) => entry.command)
    }

    return fuzzysort.searchFields(
      search,
      availableCommands,
      [
        { key: "label", extract: (entry) => entry.preparedLabel },
        { key: "keywords", extract: (entry) => entry.preparedKeywords },
      ],
      { threshold: 0 },
    ).items.map((entry) => entry.value.command)
  })()

  <DialogPortal>
    <Dialog.Backdrop class={styles.backdrop} />
    <Dialog.Positioner class={styles.positioner}>
      <Dialog.Content class={styles.content}>
        <Dialog.Title class={styles.title}>{"Command menu"}</Dialog.Title>
        <Command.Root
          class={styles.root}
          filter={() => 1}
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
            empty={<tsx>
              <div class={styles.empty}>
                {"No matching commands."}
              </div>
            </tsx>}
            children={(item, state) => {
              const Icon = item.icon
              const shortcutBinding = shortcutRegistry.resolvedBindings[item.id]?.[0]
              const shortcut =
                typeof shortcutBinding === "string"
                  ? shortcutBinding
                  : shortcutBinding?.combo ?? shortcutBinding?.sequence

              return <tsx>
                <div class={styles.item} data-active={state.active ? "true" : "false"} style={{
                  gridTemplateColumns: Icon ? "16px minmax(0, 1fr) auto" : "minmax(0, 1fr) auto",
                }}>
                  {Icon
                    ? <Icon aria-hidden={true} size={16} strokeWidth={2} style={{
                        color: state.active ? token.var("colors.text") : token.var("colors.muted"),
                      }} />
                    : null}
                  <span class={styles.itemLabel}>
                    {item.label}
                  </span>
                  {shortcut
                    ? <span class={styles.shortcut}>
                        {shortcut}
                      </span>
                    : null}
                </div>
              </tsx>
            }}
          />
        </Command.Root>
      </Dialog.Content>
    </Dialog.Positioner>
  </DialogPortal>
}

export default CommandPalette
