import { Dialog, useDialog } from "@ark-ui/react/dialog"
import { Menu } from "@ark-ui/react/menu"
import { cx } from "@goddard-ai/styled-system/css"
import { Command, Search } from "lucide-react"
import type { RecordingSession } from "powerkeys"
import { useEffect, useRef, useState } from "preact/hooks"

import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { appCommandList } from "~/commands/app-command.ts"
import { MenuPortal } from "~/lib/menu-portal.tsrx"
import type { AppCommandId } from "~/shared/app-commands.ts"
import { getShortcutBindingExpression, getShortcutBindingWhen } from "~/shared/shortcut-keymap.ts"
import { KeyboardShortcutCaptureDialog } from "./capture-dialog.tsrx"
import styles from "./view.style.ts"

function formatShortcutSearch(expression: string) {
  return `"${expression}"`
}

function getExactShortcutSearch(searchText: string) {
  const trimmedSearch = searchText.trim()

  if (!trimmedSearch.startsWith("\"") || !trimmedSearch.endsWith("\"")) {
    return null
  }

  return trimmedSearch.slice(1, -1).trim().toLowerCase()
}

export default function KeyboardShortcutsView(props: { class?: string }) {
  const shortcutRegistry = useShortcutRegistry()
  const searchInputRef = useRef<HTMLInputElement | null>(null)
  const searchRecordingSessionRef = useRef<RecordingSession | null>(null)
  const [searchText, setSearchText] = useState("")
  const [isRecordingSearchEnabled, setIsRecordingSearchEnabled] = useState(false)
  const [editingWhenRowKey, setEditingWhenRowKey] = useState<string | null>(null)
  const [captureState, setCaptureState] = useState<
    {
      commandId: AppCommandId;
      label: string;
      mode: "add" | "change";
      bindingIndex: number | null;
      currentExpression: string | null;
      whenClause: string | null;
    } | null
  >(null)
  const captureDialog = useDialog(
    {
      open: captureState !== null,
      onOpenChange: (details) => {
        if (!details.open) {
          setCaptureState(null)
        }
      },
    },
  )

  function stopSearchRecording() {
    const session = searchRecordingSessionRef.current
    searchRecordingSessionRef.current = null
    session?.cancel()
  }

  function openCaptureDialog(row: {
    commandId: AppCommandId;
    label: string;
    bindingIndex: number | null;
    shortcut: string | null;
    whenClause: string | null;
  }) {
    stopSearchRecording()
    setIsRecordingSearchEnabled(false)
    setEditingWhenRowKey(null)
    setCaptureState(
      {
        commandId: row.commandId,
        label: row.label,
        mode: row.bindingIndex === null ? "add" : "change",
        bindingIndex: row.bindingIndex,
        currentExpression: row.shortcut,
        whenClause: row.whenClause,
      },
    )
  }

  useEffect(() => {
    if (!isRecordingSearchEnabled || captureState !== null) {
      return
    }

    let isDisposed = false
    searchInputRef.current?.focus()

    function beginRecording() {
      if (isDisposed) {
        return
      }

      try {
        const session = shortcutRegistry.runtime.record(
          {
            target: searchInputRef.current ?? document,
            suppressHandlers: true,
            consumeEvents: true,
            onUpdate: (recording) => {
              if (recording.expression === "Escape") {
                stopSearchRecording()
                setIsRecordingSearchEnabled(false)
                return
              }

              setSearchText(formatShortcutSearch(recording.expression))
            },
          },
        )

        searchRecordingSessionRef.current = session

        void session.finished.then(
          (recording) => {
            if (searchRecordingSessionRef.current === session) {
              searchRecordingSessionRef.current = null
            }
            if (isDisposed) {
              return
            }
            if (recording.expression === "Escape") {
              setIsRecordingSearchEnabled(false)
              return
            }

            setSearchText(formatShortcutSearch(recording.expression))
            beginRecording()
          },
          () => {
            if (searchRecordingSessionRef.current === session) {
              searchRecordingSessionRef.current = null
            }
          },
        )
      } catch (error) {
        setIsRecordingSearchEnabled(false)
        console.error("Failed to begin shortcut search recording.", error)
      }
    }

    beginRecording()

    return () => {
      isDisposed = true
      stopSearchRecording()
    }
  }, [captureState, isRecordingSearchEnabled, shortcutRegistry.runtime])

  const allRows: Array<{
    rowKey: string;
    commandId: AppCommandId;
    label: string;
    bindingIndex: number | null;
    shortcut: string | null;
    whenClause: string | null;
  }> = []

  for (const command of appCommandList) {
    const bindings = shortcutRegistry.resolvedBindings[command.id]

    if (!bindings || bindings.length === 0) {
      allRows.push(
        {
          rowKey: `${command.id}:empty`,
          commandId: command.id,
          label: command.label,
          bindingIndex: null,
          shortcut: null,
          whenClause: command.when ?? null,
        },
      )
      continue
    }

    bindings.forEach((binding, bindingIndex) => {
      allRows.push(
        {
          rowKey: `${command.id}:${bindingIndex}`,
          commandId: command.id,
          label: command.label,
          bindingIndex,
          shortcut: getShortcutBindingExpression(binding),
          whenClause: getShortcutBindingWhen(binding) ?? command.when ?? null,
        },
      )
    })
  }

  allRows.sort((left, right) => {
    const labelSort = left.label.localeCompare(right.label)

    if (labelSort !== 0) {
      return labelSort
    }

    const shortcutSort = (left.shortcut ?? "￿").localeCompare(right.shortcut ?? "￿")

    if (shortcutSort !== 0) {
      return shortcutSort
    }

    return (left.whenClause ?? "").localeCompare(right.whenClause ?? "")
  })

  const exactShortcutSearch = getExactShortcutSearch(searchText)
  const normalizedSearch = searchText.trim().toLowerCase()
  const visibleRows = allRows.filter((row) => {
    if (exactShortcutSearch !== null) {
      return exactShortcutSearch.length === 0
        ? true
        : row.shortcut?.toLowerCase() === exactShortcutSearch
    }

    if (normalizedSearch.length === 0) {
      return true
    }

    return row.label.toLowerCase().includes(normalizedSearch) ||
      row.shortcut?.toLowerCase().includes(normalizedSearch) ||
      row.whenClause?.toLowerCase().includes(normalizedSearch)
  })

  return <tsx:react>
    <section class={cx(styles.page, props.class)}>
      <label class={styles.searchField}>
        <span class={styles.srOnly}>Search shortcuts</span>
        <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
        <input ref={searchInputRef} class={styles.searchInput} placeholder={isRecordingSearchEnabled
          ? "Type a key binding"
          : "Search commands, shortcuts, or when clauses"} readOnly={isRecordingSearchEnabled} value={searchText} onInput={(
          event,
        ) => {
          setSearchText(event.currentTarget.value)
        }} onKeyDown={(event) => {
          if (event.key === "Escape" && isRecordingSearchEnabled) {
            event.preventDefault()
            stopSearchRecording()
            setIsRecordingSearchEnabled(false)
          }
        }} />
        <button aria-label={isRecordingSearchEnabled
          ? "Stop recording key binding search"
          : "Record key binding search"} aria-pressed={isRecordingSearchEnabled} class={cx(
          styles.recordButton,
          isRecordingSearchEnabled && styles.recordButtonActive,
        )} type="button" onClick={() => {
          if (isRecordingSearchEnabled) {
            stopSearchRecording()
            setIsRecordingSearchEnabled(false)
            return
          }

          searchInputRef.current?.focus()
          setIsRecordingSearchEnabled(true)
        }}>
          <Command size={15} strokeWidth={2.2} />
        </button>
      </label>
      {shortcutRegistry.loadError
        ? <tsx:react>
            <div class={styles.warning}>
              The saved keymap could not be loaded, so the built-in profile is
          active.
              <br />
              {shortcutRegistry.loadError}
            </div>
          </tsx:react>
        : null}
      {shortcutRegistry.writeError
        ? <tsx:react>
            <div class={styles.error}>
              {shortcutRegistry.writeError}
            </div>
          </tsx:react>
        : null}
      <div class={styles.tableFrame}>
        <table class={styles.table}>
          <thead>
            <tr>
              <th class={styles.headerCell} scope="col" style={{
                width: "44%",
              }}>Command</th>
              <th class={styles.headerCell} scope="col" style={{
                width: "23%",
              }}>Shortcut</th>
              <th class={styles.headerCell} scope="col">When</th>
            </tr>
          </thead>
          <tbody>
            {visibleRows.length > 0
              ? (visibleRows.map(
                  (row) => <tsx:react>
                    <tr key={row.rowKey} class={styles.row}>
                      <td class={`${styles.cell} ${styles.commandCell}`}>
                        {row.label}
                      </td>
                      <td class={styles.cell}>
                        <Menu.Root aria-label={`${row.label} shortcut actions`} lazyMount={true} positioning={{
                          fitViewport: true,
                          placement: "bottom-start",
                        }} unmountOnExit={true}>
                          <Menu.ContextTrigger class={cx(
                            styles.cellButton,
                            styles.shortcutButton,
                            !row.shortcut && styles.cellButtonMuted,
                          )} type="button" onClick={() => {
                            openCaptureDialog(row)
                          }}>
                            {row.shortcut ?? "Add shortcut"}
                          </Menu.ContextTrigger>
                          <MenuPortal>
                            <Menu.Positioner>
                              <Menu.Content class={styles.menuContent}>
                                <Menu.Item class={styles.menuItem} value={`${row.rowKey}:add`} onSelect={() => {
                                  openCaptureDialog(
                                    {
                                      commandId: row.commandId,
                                      label: row.label,
                                      bindingIndex: null,
                                      shortcut: null,
                                      whenClause: row.whenClause,
                                    },
                                  )
                                }}>Add shortcut</Menu.Item>
                                <Menu.Item class={styles.menuItem} disabled={row.bindingIndex ===
                                  null} value={`${row.rowKey}:change`} onSelect={() => {
                                  if (row.bindingIndex === null) {
                                    return
                                  }

                                  openCaptureDialog(row)
                                }}>Change shortcut</Menu.Item>
                                <Menu.Item class={cx(
                                  styles.menuItem,
                                  styles.menuItemDanger,
                                )} disabled={row.bindingIndex ===
                                  null} value={`${row.rowKey}:remove`} onSelect={() => {
                                  if (row.bindingIndex === null) {
                                    return
                                  }

                                  setEditingWhenRowKey(null)
                                  void shortcutRegistry.removeCommandBinding(
                                    row.commandId,
                                    row.bindingIndex,
                                  )
                                }}>Remove shortcut</Menu.Item>
                                <Menu.Item class={styles.menuItem} disabled={!row.shortcut} value={`${row.rowKey}:show-matches`} onSelect={() => {
                                  if (!row.shortcut) {
                                    return
                                  }

                                  stopSearchRecording()
                                  setIsRecordingSearchEnabled(false)
                                  setSearchText(formatShortcutSearch(row.shortcut))
                                  searchInputRef.current?.focus()
                                }}>Show commands with same shortcut</Menu.Item>
                              </Menu.Content>
                            </Menu.Positioner>
                          </MenuPortal>
                        </Menu.Root>
                      </td>
                      <td class={styles.cell}>
                        {editingWhenRowKey === row.rowKey && row.bindingIndex !== null
                          ? <tsx:react>
                              <input autoFocus={true} class={styles.whenInput} defaultValue={row.whenClause ??
                                ""} onBlur={(event) => {
                                setEditingWhenRowKey(null)
                                if (row.bindingIndex === null) {
                                  return
                                }
                                void shortcutRegistry.updateCommandBindingWhen(
                                  row.commandId,
                                  row.bindingIndex,
                                  event.currentTarget.value,
                                )
                              }} onKeyDown={(event) => {
                                if (event.key === "Enter") {
                                  event.preventDefault()
                                  event.currentTarget.blur()
                                  return
                                }

                                if (event.key === "Escape") {
                                  event.preventDefault()
                                  setEditingWhenRowKey(null)
                                }
                              }} />
                            </tsx:react>
                          : row.bindingIndex !== null
                            ? <tsx:react>
                                <button class={cx(
                                  styles.cellButton,
                                  styles.whenButton,
                                  !row.whenClause && styles.cellButtonMuted,
                                )} type="button" onClick={() => {
                                  setEditingWhenRowKey(row.rowKey)
                                }}>
                                  {row.whenClause ?? "Add when clause"}
                                </button>
                              </tsx:react>
                            : <tsx:react>
                                <span class={styles.cellButtonMuted}>
                                  {row.whenClause ?? "–"}
                                </span>
                              </tsx:react>}
                      </td>
                    </tr>
                  </tsx:react>,
                ))
              : <tsx:react>
                  <tr>
                    <td class={styles.cell} colSpan={3}>
                      <div class={styles.empty}>No matching shortcuts.</div>
                    </td>
                  </tr>
                </tsx:react>}
          </tbody>
        </table>
      </div>
      <Dialog.RootProvider value={captureDialog} lazyMount unmountOnExit>
        {captureState
          ? <tsx:react>
              <KeyboardShortcutCaptureDialog currentExpression={captureState.currentExpression} dialog={captureDialog} label={captureState.label} mode={captureState.mode} whenClause={captureState.whenClause} onRemove={captureState.bindingIndex ===
              null
                ? undefined
                : async () => {
                    if (captureState.bindingIndex === null) {
                      return
                    }

                    if (
                      await shortcutRegistry.removeCommandBinding(
                        captureState.commandId,
                        captureState.bindingIndex,
                      )
                    ) {
                      setCaptureState(null)
                    }
                  }} onSave={async (expression) => {
                if (captureState.mode === "add") {
                  if (
                    await shortcutRegistry.addCommandBinding(
                      captureState.commandId,
                      expression,
                      captureState.whenClause,
                    )
                  ) {
                    setCaptureState(null)
                  }

                  return
                }

                if (captureState.bindingIndex === null) {
                  return
                }

                if (
                  await shortcutRegistry.replaceCommandBinding(
                    captureState.commandId,
                    captureState.bindingIndex,
                    expression,
                  )
                ) {
                  setCaptureState(null)
                }
              }} />
            </tsx:react>
          : null}
      </Dialog.RootProvider>
    </section>
  </tsx:react>
}
