/** Shared custom dropdown used by the launch-session selectors. */
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { type Signal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { ChevronDown, LoaderCircle } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "preact/hooks"

import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import GoodKeyboardShortcut from "~/lib/good-keyboard-shortcut.tsrx"
import { Popover } from "@goddard-ai/ui-primitives"
import { isEmptyQuery } from "~/lib/search-query.ts"
import styles from "./select-menu.style.ts"

type SelectItemIconProps = {
  class?: string;
  size?: number;
  strokeWidth?: number;
  "aria-hidden"?: boolean;
}

type SelectItemIcon = preact.FunctionComponent<SelectItemIconProps>

const selectorMenuMinimumFuzzyScore = 0.12
const selectorMenuTypeaheadTimeoutMs = 700

/** One item rendered inside a launch-session selector menu. */
export type SessionInputSelectItem = {
  value: string;
  label: string;
  detail?: string | null;
  icon?: string | null;
  Icon?: SelectItemIcon;
  searchText?: string | null;
  disabled?: boolean;
}

/** Props for one shared launch-session selector menu. */
export type SessionInputSelectProps = {
  label: string;
  placeholder: string;
  shortcut?: string | null;
  value: string | null;
  items: readonly SessionInputSelectItem[];
  open: Signal<boolean>;
  disabled?: boolean;
  filterable?: boolean;
  loading?: boolean;
  compact?: boolean;
  onValueChange: (value: string) => void;
}

function normalizeSelectorQuery(query: string) {
  return query.trim().toLocaleLowerCase()
}

function hasDirectSelectorMatch<T>(query: string, match: fuzzysort.RecordMatch<T>) {
  const normalizedQuery = normalizeSelectorQuery(query)

  return normalizedQuery.length > 0 &&
    match.fields.some((field) => field.target.toLocaleLowerCase().includes(normalizedQuery))
}

function isStrongSelectorMatch<T>(query: string, match: fuzzysort.RecordMatch<T>) {
  return match.score >= selectorMenuMinimumFuzzyScore || hasDirectSelectorMatch(query, match)
}

function isSelectorTypeaheadKey(event: KeyboardEvent) {
  return event.key.length === 1 && event.key.trim().length > 0 && !event.altKey && !event.ctrlKey &&
    !event.metaKey
}

/** Shared dropdown control used for project, adapter, branch, model, and thinking selectors. */
export component SessionInputSelect(props: SessionInputSelectProps) {
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const filterInputRef = useRef<HTMLInputElement>(null)

  const selectedItem = props.items.find((item) => item.value === props.value) ?? null
  const triggerLabel = selectedItem?.label ?? props.placeholder
  const ariaLabel = `${props.label}: ${triggerLabel}`
  const SelectedIcon = selectedItem?.Icon

  <div class={styles.field} data-compact={props.compact}>
    <GoodTooltip
      ariaLabel={props.label}
      content={<tsrx>
        <span class={styles.tooltipContent}>
          <span>
            {props.label}
          </span>
          <span class={styles.tooltipShortcut}>
            if (props.shortcut) {
              <GoodKeyboardShortcut expression={props.shortcut} />
              " opens menu"
            } else {
              "No shortcut assigned"
            }
          </span>
        </span>
      </tsrx>}
      group="session-input-selectors"
    >
      <span class={styles.tooltipTrigger}>
        <button
          {ref triggerElementRef}
          aria-expanded={props.open.value}
          aria-haspopup="menu"
          aria-label={ariaLabel}
          class={styles.trigger}
          data-compact={props.compact}
          disabled={props.disabled}
          type="button"
          onClick={() => {
            props.open.value = !props.open.value
          }}
          onKeyDown={(event) => {
            if (props.disabled) {
              return
            }

            if (event.key === "ArrowDown") {
              event.preventDefault()
              props.open.value = true
              return
            }

            if (event.key === "Escape" && props.open.value) {
              event.preventDefault()
              event.stopPropagation()
              props.open.value = false
            }
          }}
        >
          <span class={cx(styles.triggerContent, !selectedItem && styles.triggerPlaceholder)}>
            if (SelectedIcon) {
              <SelectedIcon
                aria-hidden={true}
                class={styles.itemIcon}
                size={16}
                strokeWidth={2.2}
              />
            } else if (selectedItem?.icon) {
              <img alt="" class={styles.itemIcon} src={selectedItem.icon} />
            }

            <span class={styles.triggerLabel}>
              {triggerLabel}
            </span>
          </span>
          <ChevronDown
            class={styles.chevron}
            size={16}
            strokeWidth={2.2}
            style={{
              color: props.open.value ? token.var("colors.text") : token.var("colors.muted"),
              transform: props.open.value ? "rotate(180deg)" : "rotate(0deg)",
            }}
          />
        </button>
      </span>
    </GoodTooltip>

    <Popover
      anchor={() => triggerElementRef.current}
      class={styles.menu}
      closeOnEscape={true}
      closeOnOutsidePointer={true}
      initialFocus={() => props.filterable ? filterInputRef.current : null}
      modalFocus={props.filterable ?? false}
      offset={8}
      open={props.open}
      placement="bottom-start"
      strategy="fixed"
    >
      const [query, setQuery] = useState("")
      const [selectedIndex, setSelectedIndex] = useState(0)
      const [itemElements] = useState<Record<string, HTMLButtonElement | null>>(() => ({}))
      const typeaheadQueryRef = useRef("")
      const typeaheadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
      const ignorePointerHoverRef = useRef(false)
      const lastPointerPositionRef = useRef<{ x: number; y: number } | null>(null)

      const searchableItems = useMemo(
        () => props.items.map(
          (item) => ({
            item,
            preparedLabel: fuzzysort.prepare(item.label),
          }),
        ),
        [props.items],
      )

      const filteredItems = useMemo(() => {
        if (!props.filterable || isEmptyQuery(query)) {
          return props.items
        }

        return fuzzysort.searchFields(
          query,
          searchableItems,
          [
            { key: "label", extract: (entry) => entry.preparedLabel },
          ],
          { threshold: 0 },
        ).items.filter((entry) => isStrongSelectorMatch(query, entry)).map(
          (entry) => entry.value.item,
        )
      }, [props.filterable, props.items, query, searchableItems])

      const filteredItemSignature = filteredItems.map((item) => item.value).join("\n")
      const highlightedItem = filteredItems[selectedIndex] ?? filteredItems[0] ?? null

      function getNextFocusableIndex(startIndex: number, step: 1 | -1) {
        if (filteredItems.length === 0) {
          return null
        }

        for (let offset = 1; offset <= filteredItems.length; offset += 1) {
          const index = (startIndex + step * offset + filteredItems.length) % filteredItems.length

          if (!filteredItems[index]?.disabled) {
            return index
          }
        }

        return null
      }

      function getFocusedItemIndex() {
        return filteredItems.findIndex(
          (item) => itemElements[item.value] === document.activeElement,
        )
      }

      function clearTypeaheadQuery() {
        typeaheadQueryRef.current = ""

        if (typeaheadTimerRef.current) {
          clearTimeout(typeaheadTimerRef.current)
          typeaheadTimerRef.current = null
        }
      }

      function scheduleTypeaheadReset() {
        if (typeaheadTimerRef.current) {
          clearTimeout(typeaheadTimerRef.current)
        }

        typeaheadTimerRef.current = setTimeout(clearTypeaheadQuery, selectorMenuTypeaheadTimeoutMs)
      }

      function ignorePointerHoverUntilPointerMoves() {
        ignorePointerHoverRef.current = true
      }

      function updatePointerPosition(event: PointerEvent) {
        const previousPosition = lastPointerPositionRef.current
        const nextPosition = {
          x: event.clientX,
          y: event.clientY,
        }

        if (
          !previousPosition || previousPosition.x !== nextPosition.x ||
            previousPosition.y !== nextPosition.y
        ) {
          ignorePointerHoverRef.current = false
        }

        lastPointerPositionRef.current = nextPosition
      }

      function moveFocusedItem(step: 1 | -1) {
        setSelectedIndex((currentIndex) => {
          const focusedIndex = getFocusedItemIndex()
          const nextIndex = getNextFocusableIndex(
            focusedIndex >= 0 ? focusedIndex : currentIndex,
            step,
          )

          if (nextIndex === null) {
            return filteredItems.length === 0 ? 0 : currentIndex
          }

          ignorePointerHoverUntilPointerMoves()
          queueMicrotask(() => {
            const item = filteredItems[nextIndex]
            if (item) {
              itemElements[item.value]?.focus()
            }
          })
          return nextIndex
        })
      }

      function moveHighlightedItem(step: 1 | -1) {
        setSelectedIndex((currentIndex) => {
          const nextIndex = getNextFocusableIndex(currentIndex, step)

          if (nextIndex !== null) {
            ignorePointerHoverUntilPointerMoves()
          }

          return nextIndex === null ? currentIndex : nextIndex
        })
      }

      function getSearchableItemMatch(query: string, item: SessionInputSelectItem) {
        const searchableItem = searchableItems.find((entry) => entry.item.value === item.value)

        if (!searchableItem) {
          return null
        }

        return fuzzysort.searchFields(
          query,
          [searchableItem],
          [
            { key: "label", extract: (entry) => entry.preparedLabel },
          ],
          { threshold: 0 },
        ).items[0] ??
          null
      }

      function findNearestTypeaheadIndex(query: string) {
        if (isEmptyQuery(query) || filteredItems.length === 0) {
          return null
        }

        const focusedIndex = getFocusedItemIndex()
        const currentIndex =
          focusedIndex >= 0 ? focusedIndex : selectedIndex
        const firstOffset =
          query.length > 1 ? 0 : 1

        for (let offset = firstOffset; offset < filteredItems.length + firstOffset; offset += 1) {
          const index = (currentIndex + offset) % filteredItems.length
          const item = filteredItems[index]

          if (!item || item.disabled) {
            continue
          }

          const match = getSearchableItemMatch(query, item)

          if (match && isStrongSelectorMatch(query, match)) {
            return index
          }
        }

        return null
      }

      function focusTypeaheadItem(key: string) {
        const combinedQuery = `${typeaheadQueryRef.current}${key}`
        let nextQuery = combinedQuery
        let nextIndex = findNearestTypeaheadIndex(nextQuery)

        if (nextIndex === null && typeaheadQueryRef.current) {
          nextQuery = key
          nextIndex = findNearestTypeaheadIndex(nextQuery)
        }

        typeaheadQueryRef.current = nextQuery
        scheduleTypeaheadReset()

        if (nextIndex === null) {
          return
        }

        ignorePointerHoverUntilPointerMoves()
        setSelectedIndex(nextIndex)
        queueMicrotask(() => {
          const item = filteredItems[nextIndex]
          if (item) {
            itemElements[item.value]?.focus()
          }
        })
      }

      function commitSelection(item: SessionInputSelectItem | null) {
        if (!item || item.disabled) {
          return
        }

        props.onValueChange(item.value)
        props.open.value = false
      }

      useEffect(() => {
        if (!props.open.value) {
          setQuery("")
          setSelectedIndex(0)
          clearTypeaheadQuery()
          ignorePointerHoverRef.current = false
          return
        }

        ignorePointerHoverUntilPointerMoves()
        const currentIndex = filteredItems.findIndex((item) => item.value === props.value)
        setSelectedIndex(
          props.filterable && !isEmptyQuery(query)
            ? 0
            : currentIndex >= 0
              ? currentIndex
              : 0,
        )
      }, [filteredItemSignature, props.filterable, props.open.value, props.value, query])

      useEffect(() => clearTypeaheadQuery, [])

      useEffect(() => {
        if (!props.open.value || !highlightedItem) {
          return
        }

        itemElements[highlightedItem.value]?.scrollIntoView({
          block: "nearest",
        })
      }, [highlightedItem?.value, props.open.value])

      <div
        class={styles.menuBody}
        onPointerMove={updatePointerPosition}
        onKeyDown={(event) => {
          if (props.filterable && event.target === filterInputRef.current) {
            return
          }

          if (!props.filterable && isSelectorTypeaheadKey(event)) {
            event.preventDefault()
            focusTypeaheadItem(event.key)
            return
          }

          switch (event.key) {
            case "ArrowDown":
              event.preventDefault()
              moveFocusedItem(1)
              return
            case "ArrowUp":
              event.preventDefault()
              moveFocusedItem(-1)
              return
            case "Enter":
              event.preventDefault()
              commitSelection(highlightedItem)
              return
            case "Escape":
              event.preventDefault()
              event.stopPropagation()
              props.open.value = false
          }
        }}
      >
        if (props.filterable) {
          <input
            {ref filterInputRef}
            class={styles.menuFilter}
            placeholder="Type to filter"
            value={query}
            onInput={(event) => {
              setQuery(event.currentTarget.value)
              setSelectedIndex(0)
            }}
            onKeyDown={(event) => {
              switch (event.key) {
                case "ArrowDown":
                  event.preventDefault()
                  moveHighlightedItem(1)
                  return
                case "ArrowUp":
                  event.preventDefault()
                  moveHighlightedItem(-1)
                  return
                case "Enter":
                  event.preventDefault()
                  commitSelection(highlightedItem)
                  return
                case "Escape":
                  event.preventDefault()
                  event.stopPropagation()
                  props.open.value = false
              }
            }}
          />
        }
        if (props.loading) {
          <div class={styles.menuEmpty}>
            <span
              class={css({
                display: "inline-flex",
                alignItems: "center",
                gap: "8px",
              })}
            >
              <LoaderCircle class={css({ animation: "spin 1s linear infinite" })} size={14} />
              "Loading options..."
            </span>
          </div>
        } else if (filteredItems.length === 0) {
          <div class={styles.menuEmpty}>"No matching options."</div>
        } else {
          <ul class={styles.menuList}>
            for (const item of filteredItems; index index) {
              const ItemIcon = item.Icon

              <li key={item.value}>
                <button
                  {ref (element) => {
                    itemElements[item.value] = element
                  }}
                  class={cx(styles.menuButton, index === selectedIndex && styles.menuButtonActive)}
                  disabled={item.disabled}
                  type="button"
                  onMouseEnter={() => {
                    if (ignorePointerHoverRef.current) {
                      return
                    }

                    setSelectedIndex(index)
                  }}
                  onFocus={() => {
                    setSelectedIndex(index)
                  }}
                  onClick={() => {
                    commitSelection(item)
                  }}
                >
                  if (ItemIcon) {
                    <ItemIcon
                      aria-hidden={true}
                      class={styles.itemIcon}
                      size={16}
                      strokeWidth={2.2}
                    />
                  } else if (item.icon) {
                    <img alt="" class={styles.itemIcon} src={item.icon} />
                  }

                  <span class={styles.menuButtonText}>
                    <span class={styles.menuButtonLabel}>
                      {item.label}
                    </span>

                    if (item.detail) {
                      <span class={styles.menuButtonDetail}>
                        {item.detail}
                      </span>
                    }
                  </span>
                </button>
              </li>
            }
          </ul>
        }
      </div>
    </Popover>
  </div>
}
