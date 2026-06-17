import { type Signal, useSignal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useEffect, useId } from "preact/hooks"

import { text } from "~/language/text.ts"
import { useShortcutRegistry } from "./app-state-context.tsrx"
import styles from "./command-palette.style.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandPaletteVisible } from "./commands/command-context.ts"
import { GoodKeyboardShortcut } from "./lib/good-keyboard-shortcut.tsrx"
import { Modal, useSearchNavigation } from "@goddard-ai/ui-primitives"
import { isEmptyQuery } from "./lib/search-query.ts"
import { getShortcutBindingExpression } from "./shared/shortcut-keymap.ts"

const preparedAppCommands = appCommandList.map(
  (command) => ({
    command,
    preparedKeywords: command.keywords ? fuzzysort.prepare(command.keywords.join("\n")) : null,
    preparedLabel: fuzzysort.prepare(command.label),
  }),
)

export default function CommandPalette(props: { open: Signal<boolean> }) @{
  const shortcutRegistry = useShortcutRegistry()
  const titleId = useId()
  const listboxId = useId()
  const inputId = useId()
  const search = useSignal("")

  const visibleCommands = (() => {
    const availableCommands = preparedAppCommands.filter(
      (entry) =>
        entry.command.id !== "navigation.openCommandPalette" &&
          isCommandPaletteVisible(shortcutRegistry.runtime, entry.command),
    )

    if (isEmptyQuery(search.value)) {
      return availableCommands.map((entry) => entry.command)
    }

    return fuzzysort.searchFields(
      search.value,
      availableCommands,
      [
        { key: "label", extract: (entry) => entry.preparedLabel },
        { key: "keywords", extract: (entry) => entry.preparedKeywords },
      ],
      { threshold: 0 },
    ).items.map((entry) => entry.value.command)
  })()

  const visibleCommandIds = visibleCommands.map((command) => command.id).join("\n")

  const selectCommand = (command: AppCommand) => {
    props.open.value = false
    window.setTimeout(() => {
      command()
    }, 0)
  }

  const navigation = useSearchNavigation({
    activeAttribute: "aria-selected",
    count: () => visibleCommands.length,
    onActivate(index) {
      const command = visibleCommands[index]

      if (command) {
        selectCommand(command)
      }
    },
    onQueryChange(query) {
      search.value = query
    },
  })

  useEffect(() => {
    if (!props.open.value && search.value.length > 0) {
      search.value = ""
    }

    navigation.resetActiveIndex()
  }, [props.open.value, search.value, visibleCommandIds])

  <Modal
    open={props.open}
    {titleId}
    backdropClass={styles.backdrop}
    positionerClass={styles.positioner}
    contentClass={styles.content}
  >
    <h2 id={titleId} class={styles.title}>{text.commandMenu}</h2>

    <div
      class={styles.root}
      role="combobox"
      aria-controls={listboxId}
      aria-expanded={props.open.value}
      aria-haspopup="listbox"
      aria-labelledby={titleId}
      aria-owns={listboxId}
    >
      <div class={styles.searchRow}>
        <Search aria-hidden={true} class={styles.searchIcon} size={16} />
        <input
          ref={navigation.inputRef}
          id={inputId}
          autoFocus={true}
          class={styles.input}
          placeholder={text.typeACommand}
          role="searchbox"
          type="text"
          value={search.value}
          aria-autocomplete="list"
          aria-controls={listboxId}
        />
      </div>
      <div id={listboxId} class={styles.list} role="listbox" aria-label={text.commandMenu}>
        @if (visibleCommands.length === 0) {
          <div class={styles.empty}>{text.noMatchingCommands}</div>
        } @else {
          @for (const command of visibleCommands; index index) {
            const itemId = `${listboxId}-${command.id}`
            const Icon = command.icon
            const shortcut = getShortcutBindingExpression(
              shortcutRegistry.resolvedBindings[command.id]?.[0],
            )

            <div
              id={itemId}
              key={command.id}
              ref={navigation.itemRef(index)}
              class={styles.item}
              role="option"
              aria-selected="false"
              style={{
                gridTemplateColumns: Icon ? "16px minmax(0, 1fr) auto" : "minmax(0, 1fr) auto",
              }}
              onMouseDown={(event) => {
                event.preventDefault()
              }}
              onClick={() => {
                selectCommand(command)
              }}
            >
              @if (Icon) {
                <span class={styles.itemIcon}>
                  <Icon aria-hidden={true} size={16} strokeWidth={2} />
                </span>
              }

              <span class={styles.itemLabel}>{command.label}</span>

              @if (shortcut) {
                <GoodKeyboardShortcut class={styles.shortcut} expression={shortcut} />
              }
            </div>
          }
        }
      </div>
    </div>
  </Modal>
}
