/** Renders the app-wide command palette. */
import { token } from "@goddard-ai/styled-system/tokens"
import { type Signal } from "@preact/signals"
import { Command } from "ark-cmdk"
import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useEffect, useId, useState } from "preact/hooks"

import { useShortcutRegistry } from "./app-state-context.tsrx"
import styles from "./command-palette.style.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandAvailable } from "./commands/command-context.ts"
import { Modal } from "./lib/modal.tsrx"
import { isEmptyQuery } from "./lib/search-query.ts"

const preparedAppCommands = appCommandList.map(
  (command) => ({
    command,
    preparedKeywords: command.keywords ? fuzzysort.prepare(command.keywords.join("\n")) : null,
    preparedLabel: fuzzysort.prepare(command.label),
  }),
)

component CommandPalette(props: { open: Signal<boolean> }) {
  const isOpen = props.open.value
  const shortcutRegistry = useShortcutRegistry()
  const titleId = useId()

  const [search, setSearch] = useState("")
  useEffect(() => {
    if (!isOpen && search.length > 0) {
      setSearch("")
    }
  }, [isOpen, search])

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

  <Modal
    open={props.open}
    {titleId}
    backdropClass={styles.backdrop}
    positionerClass={styles.positioner}
    contentClass={styles.content}
  >
    <h2 id={titleId} class={styles.title}>"Command menu"</h2>
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
        props.open.value = false
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
        empty={<tsrx>
          <div class={styles.empty}>"No matching commands."</div>
        </tsrx>}
        children={(item, state) => {
          const command = item as AppCommand
          const Icon = command.icon
          const shortcutBinding = shortcutRegistry.resolvedBindings[command.id]?.[0]
          const shortcut =
            typeof shortcutBinding === "string"
              ? shortcutBinding
              : shortcutBinding?.combo ?? shortcutBinding?.sequence

          return <tsrx>
            <div
              class={styles.item}
              data-active={state.active ? "true" : "false"}
              style={{
                gridTemplateColumns: Icon ? "16px minmax(0, 1fr) auto" : "minmax(0, 1fr) auto",
              }}
            >
              if (Icon) {
                <Icon
                  aria-hidden={true}
                  size={16}
                  strokeWidth={2}
                  style={{
                    color: state.active ? token.var("colors.text") : token.var("colors.muted"),
                  }}
                />
              }
              <span class={styles.itemLabel}>
                {command.label}
              </span>
              if (shortcut) {
                <span class={styles.shortcut}>
                  {shortcut}
                </span>
              }
            </div>
          </tsrx>
        }}
      />
    </Command.Root>
  </Modal>
}

export default CommandPalette
