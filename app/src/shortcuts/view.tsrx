import { cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import { Command, Search } from "lucide-react"
import type { RecordingSession } from "powerkeys"
import { useEffect, useRef, useState } from "preact/hooks"

import { shortcutPersistenceErrors } from "~/app-state-persistence.ts"
import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { appCommandGroupList, appCommandList } from "~/commands/app-command.ts"
import { Menu, MenuItem } from "@goddard-ai/ui-primitives"
import GoodKeyboardShortcut from "~/lib/good-keyboard-shortcut.tsrx"
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

type CaptureState = {
  commandId: AppCommandId;
  label: string;
  mode: "add" | "change";
  bindingIndex: number | null;
  currentExpression: string | null;
  whenClause: string | null;
}

type ShortcutRow = {
  rowKey: string;
  commandId: AppCommandId;
  group: string;
  label: string;
  bindingIndex: number | null;
  shortcut: string | null;
  whenClause: string | null;
}

type ShortcutTableRow =
  | {
      kind: "group";
      key: string;
      label: string;
    }
  | {
      kind: "shortcut";
      key: string;
      row: ShortcutRow;
    }

function KeyboardShortcutsView(props: { class?: string }) @{
  const shortcutRegistry = useShortcutRegistry()
  const persistenceErrors = shortcutPersistenceErrors.value
  const searchInputRef = useRef<HTMLInputElement>(null)
  const searchRecordingSessionRef = useRef<RecordingSession | null>(null)
  const [searchText, setSearchText] = useState("")
  const [isRecordingSearchEnabled, setIsRecordingSearchEnabled] = useState(false)
  const [editingWhenRowKey, setEditingWhenRowKey] = useState<string | null>(null)
  const [captureState, setCaptureState] = useState<CaptureState | null>(null)
  const isCaptureOpen = useSignal(false)

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
    setCaptureState({
      commandId: row.commandId,
      label: row.label,
      mode: row.bindingIndex === null ? "add" : "change",
      bindingIndex: row.bindingIndex,
      currentExpression: row.shortcut,
      whenClause: row.whenClause,
    })
    isCaptureOpen.value = true
  }

  function closeCaptureDialog() {
    isCaptureOpen.value = false
    setCaptureState(null)
  }

  useEffect(() => {
    if (!isCaptureOpen.value && captureState !== null) {
      setCaptureState(null)
    }
  }, [captureState, isCaptureOpen.value])

  useEffect(() => {
    if (!isRecordingSearchEnabled || captureState !== null) {
      return
    }

    let isDisposed = false
    const searchInputElement = searchInputRef.current
    searchInputElement?.focus()

    function beginRecording() {
      if (isDisposed) {
        return
      }

      try {
        const session = shortcutRegistry.runtime.record({
          target: searchInputElement ?? document,
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
        })

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

  const allRows: ShortcutRow[] = []

  for (const command of appCommandList) {
    const bindings = shortcutRegistry.resolvedBindings[command.id]

    if (!bindings || bindings.length === 0) {
      allRows.push({
        rowKey: `${command.id}:empty`,
        commandId: command.id,
        group: command.group,
        label: command.label,
        bindingIndex: null,
        shortcut: null,
        whenClause: command.when ?? null,
      })
    } else {
      bindings.forEach((binding, bindingIndex) => {
        allRows.push({
          rowKey: `${command.id}:${bindingIndex}`,
          commandId: command.id,
          group: command.group,
          label: command.label,
          bindingIndex,
          shortcut: getShortcutBindingExpression(binding),
          whenClause: getShortcutBindingWhen(binding) ?? command.when ?? null,
        })
      })
    }
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
  const visibleGroups = appCommandGroupList.map(
    (group) => ({
      ...group,
      rows: visibleRows.filter((row) => row.group === group.id),
    }),
  ).filter((group) => group.rows.length > 0)
  const visibleTableRows: ShortcutTableRow[] = []

  for (const group of visibleGroups) {
    visibleTableRows.push({
      kind: "group",
      key: `group:${group.id}`,
      label: group.label,
    })

    for (const row of group.rows) {
      visibleTableRows.push({
        kind: "shortcut",
        key: row.rowKey,
        row,
      })
    }
  }

  <section class={cx(styles.page, props.class)}>
    <label class={styles.searchField}>
      <span class={styles.srOnly}>Search shortcuts</span>
      <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
      <input
        ref={searchInputRef}
        class={styles.searchInput}
        placeholder={isRecordingSearchEnabled
          ? "Type a key binding"
          : "Search commands, shortcuts, or when clauses"}
        readOnly={isRecordingSearchEnabled}
        value={searchText}
        onInput={(event) => {
          setSearchText(event.currentTarget.value)
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape" && isRecordingSearchEnabled) {
            event.preventDefault()
            stopSearchRecording()
            setIsRecordingSearchEnabled(false)
          }
        }}
      />
      <button
        aria-label={isRecordingSearchEnabled
          ? "Stop recording key binding search"
          : "Record key binding search"}
        aria-pressed={isRecordingSearchEnabled}
        class={cx(styles.recordButton, isRecordingSearchEnabled && styles.recordButtonActive)}
        type="button"
        onClick={() => {
          if (isRecordingSearchEnabled) {
            stopSearchRecording()
            setIsRecordingSearchEnabled(false)
            return
          }

          searchInputRef.current?.focus()
          setIsRecordingSearchEnabled(true)
        }}
      >
        <Command size={15} strokeWidth={2.2} />
      </button>
    </label>

    @if (persistenceErrors.loadError) {
      <div class={styles.warning}>
        The saved shortcut keymap could not be loaded, so the built-in profile is active.
        <br />
        {persistenceErrors.loadError}
      </div>
    }
    @if (persistenceErrors.writeError) {
      <div class={styles.error}>{persistenceErrors.writeError}</div>
    }

    <div class={styles.tableFrame}>
      <table class={styles.table}>
        <thead>
          <tr>
            <th
              class={styles.headerCell}
              scope="col"
              style={{
                width: "44%",
              }}
            >Command</th>
            <th
              class={styles.headerCell}
              scope="col"
              style={{
                width: "23%",
              }}
            >Shortcut</th>
            <th class={styles.headerCell} scope="col">When</th>
          </tr>
        </thead>
        <tbody>
          @if (visibleTableRows.length > 0) {
            @for (const tableRow of visibleTableRows; key tableRow.key) {
              <KeyboardShortcutTableRow
                editingWhenRowKey={editingWhenRowKey}
                searchInputRef={searchInputRef}
                setEditingWhenRowKey={setEditingWhenRowKey}
                setIsRecordingSearchEnabled={setIsRecordingSearchEnabled}
                setSearchText={setSearchText}
                shortcutRegistry={shortcutRegistry}
                stopSearchRecording={stopSearchRecording}
                tableRow={tableRow}
                onOpenCaptureDialog={openCaptureDialog}
              />
            }
          } @else {
            <tr>
              <td class={styles.cell} colSpan={3}>
                <div class={styles.empty}>No matching shortcuts.</div>
              </td>
            </tr>
          }
        </tbody>
      </table>
    </div>

    @if (captureState) {
      <KeyboardShortcutCaptureDialog
        currentExpression={captureState.currentExpression}
        open={isCaptureOpen}
        label={captureState.label}
        mode={captureState.mode}
        whenClause={captureState.whenClause}
        onRemove={captureState.bindingIndex === null
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
                closeCaptureDialog()
              }
            }}
        onSave={async (expression) => {
          if (captureState.mode === "add") {
            if (
              await shortcutRegistry.addCommandBinding(
                captureState.commandId,
                expression,
                captureState.whenClause,
              )
            ) {
              closeCaptureDialog()
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
            closeCaptureDialog()
          }
        }}
      />
    }
  </section>
}

function KeyboardShortcutTableRow(props: {
  editingWhenRowKey: string | null;
  searchInputRef: { current: HTMLInputElement | null };
  setEditingWhenRowKey: (value: string | null) => void;
  setIsRecordingSearchEnabled: (value: boolean) => void;
  setSearchText: (value: string) => void;
  shortcutRegistry: ReturnType<typeof useShortcutRegistry>;
  stopSearchRecording: () => void;
  tableRow: ShortcutTableRow;
  onOpenCaptureDialog: (row: {
    commandId: AppCommandId;
    label: string;
    bindingIndex: number | null;
    shortcut: string | null;
    whenClause: string | null;
  }) => void;
}) @{
  const tableRow = props.tableRow

  @if (tableRow.kind === "group") {
    <tr class={styles.groupRow}>
      <th class={styles.groupCell} colSpan={3} scope="colgroup">{tableRow.label}</th>
    </tr>
  } @else {
    <KeyboardShortcutRow
      editingWhenRowKey={props.editingWhenRowKey}
      row={tableRow.row}
      searchInputRef={props.searchInputRef}
      setEditingWhenRowKey={props.setEditingWhenRowKey}
      setIsRecordingSearchEnabled={props.setIsRecordingSearchEnabled}
      setSearchText={props.setSearchText}
      shortcutRegistry={props.shortcutRegistry}
      stopSearchRecording={props.stopSearchRecording}
      onOpenCaptureDialog={props.onOpenCaptureDialog}
    />
  }
}

function KeyboardShortcutRow(props: {
  editingWhenRowKey: string | null;
  row: ShortcutRow;
  searchInputRef: { current: HTMLInputElement | null };
  setEditingWhenRowKey: (value: string | null) => void;
  setIsRecordingSearchEnabled: (value: boolean) => void;
  setSearchText: (value: string) => void;
  shortcutRegistry: ReturnType<typeof useShortcutRegistry>;
  stopSearchRecording: () => void;
  onOpenCaptureDialog: (row: {
    commandId: AppCommandId;
    label: string;
    bindingIndex: number | null;
    shortcut: string | null;
    whenClause: string | null;
  }) => void;
}) @{
  const menuOpen = useSignal(false)
  const menuTriggerRef = useRef<HTMLButtonElement | null>(null)
  const menuContextPoint = useSignal<{ x: number; y: number } | null>(null)
  const row = props.row

  const openShortcutMenu = (point: { x: number; y: number } | null) => {
    menuContextPoint.value = point
    menuOpen.value = true
  }

  <tr class={styles.row}>
    <td class={`${styles.cell} ${styles.commandCell}`}>{row.label}</td>
    <td class={styles.cell}>
      <button
        ref={menuTriggerRef}
        class={cx(
          styles.cellButton,
          styles.shortcutButton,
          !row.shortcut && styles.cellButtonMuted,
        )}
        type="button"
        onClick={() => {
          props.onOpenCaptureDialog(row)
        }}
        onContextMenu={(event) => {
          event.preventDefault()
          openShortcutMenu({
            x: event.clientX,
            y: event.clientY,
          })
        }}
        onKeyDown={(event) => {
          if (event.key === "ContextMenu" || event.key === "F10" && event.shiftKey) {
            event.preventDefault()
            openShortcutMenu(null)
          }
        }}
      >
        @if (row.shortcut) {
          <GoodKeyboardShortcut expression={row.shortcut} />
        } @else {
          <>Add shortcut</>
        }
      </button>

      <Menu
        anchor={() => {
          const point = menuContextPoint.value

          if (point) {
            return point
          }

          return menuTriggerRef.current
        }}
        ariaLabel={`${row.label} shortcut actions`}
        class={styles.menuContent}
        open={menuOpen}
      >
        <MenuItem
          class={styles.menuItem}
          onSelect={() => {
            props.onOpenCaptureDialog({
              commandId: row.commandId,
              label: row.label,
              bindingIndex: null,
              shortcut: null,
              whenClause: row.whenClause,
            })
          }}
        >Add shortcut</MenuItem>

        <MenuItem
          class={styles.menuItem}
          disabled={row.bindingIndex === null}
          onSelect={() => {
            if (row.bindingIndex === null) {
              return
            }

            props.onOpenCaptureDialog(row)
          }}
        >Change shortcut</MenuItem>

        <MenuItem
          class={cx(styles.menuItem, styles.menuItemDanger)}
          disabled={row.bindingIndex === null}
          onSelect={() => {
            if (row.bindingIndex === null) {
              return
            }

            props.setEditingWhenRowKey(null)
            void props.shortcutRegistry.removeCommandBinding(row.commandId, row.bindingIndex)
          }}
        >Remove shortcut</MenuItem>

        <MenuItem
          class={styles.menuItem}
          disabled={!row.shortcut}
          onSelect={() => {
            if (!row.shortcut) {
              return
            }

            props.stopSearchRecording()
            props.setIsRecordingSearchEnabled(false)
            props.setSearchText(formatShortcutSearch(row.shortcut))
            props.searchInputRef.current?.focus()
          }}
        >Show commands with same shortcut</MenuItem>
      </Menu>
    </td>
    <td class={styles.cell}>
      @if (props.editingWhenRowKey === row.rowKey && row.bindingIndex !== null) {
        <input
          autoFocus={true}
          class={styles.whenInput}
          defaultValue={row.whenClause ?? ""}
          onBlur={(event) => {
            props.setEditingWhenRowKey(null)
            if (row.bindingIndex === null) {
              return
            }
            void props.shortcutRegistry.updateCommandBindingWhen(
              row.commandId,
              row.bindingIndex,
              event.currentTarget.value,
            )
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault()
              event.currentTarget.blur()
              return
            }

            if (event.key === "Escape") {
              event.preventDefault()
              props.setEditingWhenRowKey(null)
            }
          }}
        />
      } @else if (row.bindingIndex !== null) {
        <button
          class={cx(
            styles.cellButton,
            styles.whenButton,
            !row.whenClause && styles.cellButtonMuted,
          )}
          type="button"
          onClick={() => {
            props.setEditingWhenRowKey(row.rowKey)
          }}
        >
          {row.whenClause ?? "Add when clause"}
        </button>
      } @else {
        <span class={styles.cellButtonMuted}>
          {row.whenClause ?? "–"}
        </span>
      }
    </td>
  </tr>
}

export default KeyboardShortcutsView
