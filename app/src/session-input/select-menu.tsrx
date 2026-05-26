/** Shared custom dropdown used by the launch-session selectors. */
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { type Signal } from "@preact/signals"
import * as fuzzysort from "fuzzysort2"
import { ChevronDown, LoaderCircle } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "preact/hooks"

import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { Popover } from "~/lib/popover.tsrx"
import { isEmptyQuery } from "~/lib/search-query.ts"
import styles from "./select-menu.style.ts"

/** One item rendered inside a launch-session selector menu. */
export type SessionInputSelectItem = {
  value: string;
  label: string;
  detail?: string | null;
  icon?: string | null;
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
  onValueChange: (value: string) => void;
}

/** Shared dropdown control used for project, adapter, branch, model, and thinking selectors. */
export component SessionInputSelect(props: SessionInputSelectProps) {
  const triggerElementRef = useRef<HTMLButtonElement>(null)
  const filterInputRef = useRef<HTMLInputElement>(null)

  const selectedItem = props.items.find((item) => item.value === props.value) ?? null
  const shortcutLabel = props.shortcut ? `${props.shortcut} opens menu` : "No shortcut assigned"
  const triggerLabel = selectedItem?.label ?? props.placeholder
  const ariaLabel = `${props.label}: ${triggerLabel}`

  <div class={styles.field}>
    <GoodTooltip
      ariaLabel={props.label}
      content={<tsrx>
        <span class={styles.tooltipContent}>
          <span>
            {props.label}
          </span>
          <span class={styles.tooltipShortcut}>
            {shortcutLabel}
          </span>
        </span>
      </tsrx>}
    >
      <span class={styles.tooltipTrigger}>
        <button
          {ref triggerElementRef}
          aria-expanded={props.open.value}
          aria-haspopup="menu"
          aria-label={ariaLabel}
          class={styles.trigger}
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
            if (selectedItem?.icon) {
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
      sameWidth={true}
      strategy="fixed"
    >
      const [query, setQuery] = useState("")
      const [selectedIndex, setSelectedIndex] = useState(0)
      const [itemElements] = useState<Record<string, HTMLButtonElement | null>>(() => ({}))

      const searchableItems = useMemo(
        () => props.items.map(
          (item) => ({
            item,
            preparedDetail: item.detail ? fuzzysort.prepare(item.detail) : null,
            preparedLabel: fuzzysort.prepare(item.label),
            preparedSearchText: item.searchText ? fuzzysort.prepare(item.searchText) : null,
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
            { key: "detail", extract: (entry) => entry.preparedDetail },
            { key: "searchText", extract: (entry) => entry.preparedSearchText },
          ],
          { threshold: 0 },
        ).items.map((entry) => entry.value.item)
      }, [props.filterable, props.items, query, searchableItems])

      const filteredItemSignature = filteredItems.map((item) => item.value).join("\n")
      const highlightedItem = filteredItems[selectedIndex] ?? filteredItems[0] ?? null

      function moveHighlightedIndex(step: 1 | -1) {
        if (filteredItems.length === 0) {
          setSelectedIndex(0)
          return
        }

        setSelectedIndex((currentIndex) => {
          const nextIndex = currentIndex + step

          if (nextIndex < 0) {
            return filteredItems.length - 1
          }

          if (nextIndex >= filteredItems.length) {
            return 0
          }

          return nextIndex
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
          return
        }

        const currentIndex = filteredItems.findIndex((item) => item.value === props.value)
        setSelectedIndex(
          props.filterable && !isEmptyQuery(query)
            ? 0
            : currentIndex >= 0
              ? currentIndex
              : 0,
        )
      }, [filteredItemSignature, props.filterable, props.open.value, props.value, query])

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
        onKeyDown={(event) => {
          if (props.filterable) {
            return
          }

          switch (event.key) {
            case "ArrowDown":
              event.preventDefault()
              moveHighlightedIndex(1)
              return
            case "ArrowUp":
              event.preventDefault()
              moveHighlightedIndex(-1)
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
                  moveHighlightedIndex(1)
                  return
                case "ArrowUp":
                  event.preventDefault()
                  moveHighlightedIndex(-1)
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
              <li key={item.value}>
                <button
                  {ref (element) => {
                    itemElements[item.value] = element
                  }}
                  class={cx(styles.menuButton, index === selectedIndex && styles.menuButtonActive)}
                  disabled={item.disabled}
                  type="button"
                  onMouseEnter={() => {
                    setSelectedIndex(index)
                  }}
                  onClick={() => {
                    commitSelection(item)
                  }}
                >
                  if (item.icon) {
                    <img alt="" class={styles.itemIcon} src={item.icon} />
                  }
                  <span class={styles.menuButtonLabel}>
                    {item.label}
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
