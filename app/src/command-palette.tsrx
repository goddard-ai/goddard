import { token } from "@goddard-ai/styled-system/tokens"
import { type Signal, useSignal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { Search } from "lucide-react"
import { useEffect, useId, useLayoutEffect, useRef } from "preact/hooks"

import { useShortcutRegistry } from "./app-state-context.tsrx"
import styles from "./command-palette.style.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"
import { isCommandPaletteVisible } from "./commands/command-context.ts"
import GoodKeyboardShortcut from "./lib/good-keyboard-shortcut.tsrx"
import { Modal } from "@goddard-ai/ui-primitives"
import { isEmptyQuery } from "./lib/search-query.ts"

const preparedAppCommands = appCommandList.map(
  (command) => ({
    command,
    preparedKeywords: command.keywords ? fuzzysort.prepare(command.keywords.join("\n")) : null,
    preparedLabel: fuzzysort.prepare(command.label),
  }),
)

component CommandPalette(props: { open: Signal<boolean> }) {
  const shortcutRegistry = useShortcutRegistry()
  const titleId = useId()
  const listboxId = useId()
  const search = useSignal("")
  const activeIndex = useSignal(0)
  const itemRefs = useRef<Record<string, HTMLDivElement | null>>({})

  useEffect(() => {
    if (!props.open.value && search.value.length > 0) {
      search.value = ""
    }
  }, [props.open.value, search.value])

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
  const activeCommand = visibleCommands[activeIndex.value] ?? visibleCommands[0] ?? null

  useEffect(() => {
    activeIndex.value = 0
  }, [props.open.value, visibleCommandIds])

  useLayoutEffect(() => {
    if (!activeCommand) {
      return
    }

    itemRefs.current[activeCommand.id]?.scrollIntoView({ block: "nearest" })
  }, [activeCommand?.id])

  <Modal
    open={props.open}
    {titleId}
    backdropClass={styles.backdrop}
    positionerClass={styles.positioner}
    contentClass={styles.content}
  >
    <h2 id={titleId} class={styles.title}>"Command menu"</h2>

    const selectCommand = (command: AppCommand) => {
      props.open.value = false
      window.setTimeout(() => {
        command()
      }, 0)
    }

    const moveActiveIndex = (delta: number) => {
      if (visibleCommands.length === 0) {
        return
      }

      activeIndex.value = (activeIndex.value + delta + visibleCommands.length) %
      visibleCommands.length
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      switch (event.key) {
        case "ArrowDown":
          event.preventDefault()
          moveActiveIndex(1)
          return
        case "ArrowUp":
          event.preventDefault()
          moveActiveIndex(-1)
          return
        case "Home":
          event.preventDefault()
          activeIndex.value = 0
          return
        case "End":
          event.preventDefault()
          activeIndex.value = Math.max(visibleCommands.length - 1, 0)
          return
        case "Enter":
          if (activeCommand) {
            event.preventDefault()
            selectCommand(activeCommand)
          }
      }
    }

    <div
      class={styles.root}
      role="combobox"
      aria-controls={listboxId}
      aria-expanded={props.open.value}
      aria-haspopup="listbox"
      aria-labelledby={titleId}
      aria-owns={listboxId}
      onKeyDown={handleKeyDown}
    >
      <div class={styles.searchRow}>
        const inputId = useId()

        <Search aria-hidden={true} class={styles.searchIcon} size={16} />
        <input
          id={inputId}
          autoFocus={true}
          class={styles.input}
          placeholder="Type a command"
          role="searchbox"
          type="text"
          value={search.value}
          aria-activedescendant={activeCommand ? `${listboxId}-${activeCommand.id}` : undefined}
          aria-autocomplete="list"
          aria-controls={listboxId}
          onInput={(event) => {
            search.value = event.currentTarget.value
            activeIndex.value = 0
          }}
        />
      </div>
      <div id={listboxId} class={styles.list} role="listbox" aria-label="Command menu">
        if (visibleCommands.length === 0) {
          <div class={styles.empty}>"No matching commands."</div>
        } else {
          for (const command of visibleCommands) {
            const isActive = activeCommand?.id === command.id
            const itemId = `${listboxId}-${command.id}`
            const Icon = command.icon
            const shortcutBinding = shortcutRegistry.resolvedBindings[command.id]?.[0]
            const shortcut =
              typeof shortcutBinding === "string"
                ? shortcutBinding
                : shortcutBinding?.combo ?? shortcutBinding?.sequence

            <div
              id={itemId}
              key={command.id}
              ref={(element) => {
                itemRefs.current[command.id] = element
              }}
              class={styles.item}
              role="option"
              aria-selected={isActive}
              data-active={isActive ? "true" : "false"}
              style={{
                gridTemplateColumns: Icon ? "16px minmax(0, 1fr) auto" : "minmax(0, 1fr) auto",
              }}
              onMouseDown={(event) => {
                event.preventDefault()
              }}
              onMouseEnter={() => {
                activeIndex.value = visibleCommands.indexOf(command)
              }}
              onClick={() => {
                selectCommand(command)
              }}
            >
              if (Icon) {
                <Icon
                  aria-hidden={true}
                  size={16}
                  strokeWidth={2}
                  style={{
                    color: isActive ? token.var("colors.text") : token.var("colors.muted"),
                  }}
                />
              }

              <span class={styles.itemLabel}>
                {command.label}
              </span>

              if (shortcut) {
                <GoodKeyboardShortcut class={styles.shortcut} expression={shortcut} />
              }
            </div>
          }
        }
      </div>
    </div>
  </Modal>
}

export default CommandPalette
