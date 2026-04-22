/** Shared custom dropdown used by the launch-session selectors. */
import { Popover } from "@ark-ui/react/popover"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { ChevronDown, LoaderCircle } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { MenuPortal } from "~/lib/menu-portal.tsrx"
import styles from "./select-menu.style.ts"

/** One item rendered inside a launch-session selector menu. */
export type SessionInputSelectItem = {
  value: string;
  label: string;
  detail?: string | null;
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
  open: boolean;
  disabled?: boolean;
  filterable?: boolean;
  loading?: boolean;
  onOpenChange: (open: boolean) => void;
  onValueChange: (value: string) => void;
}

/** Shared dropdown control used for project, adapter, branch, model, and thinking selectors. */
export component SessionInputSelect(props: SessionInputSelectProps) {
  let menuElement: HTMLDivElement | null = null
  let filterInputElement: HTMLInputElement | null = null
  let triggerElement: HTMLButtonElement | null = null

  const isMenuOpen = props.open
  const isFilterable = props.filterable ?? false
  const selectedItem = props.items.find((item) => item.value === props.value) ?? null
  const shortcutLabel = props.shortcut ? `${props.shortcut} opens menu` : "No shortcut assigned"
  const triggerLabel = selectedItem?.label ?? props.placeholder
  const ariaLabel = `${props.label}: ${triggerLabel}`

  <Popover.Root
    autoFocus={true}
    closeOnInteractOutside={true}
    finalFocusEl={() => triggerElement}
    immediate={true}
    initialFocusEl={() => isFilterable ? filterInputElement : menuElement}
    lazyMount={true}
    modal={isFilterable}
    open={isMenuOpen}
    positioning={{
      fitViewport: true,
      getAnchorElement: () => triggerElement,
      gutter: 8,
      placement: "bottom-start",
      sameWidth: true,
      strategy: "fixed",
    }}
    unmountOnExit={true}
    onOpenChange={(details) => {
      props.onOpenChange(details.open)
    }}
  >
    <div class={styles.field}>
      <GoodTooltip
        ariaLabel={props.label}
        content={<tsx>
          <span class={styles.tooltipContent}>
            <span>
              {props.label}
            </span>
            <span class={styles.tooltipShortcut}>
              {shortcutLabel}
            </span>
          </span>
        </tsx>}
      >
        <span class={styles.tooltipTrigger}>
          <Popover.Trigger asChild>
            <button
              {ref triggerElement}
              aria-expanded={isMenuOpen}
              aria-haspopup="menu"
              aria-label={ariaLabel}
              class={styles.trigger}
              disabled={props.disabled}
              type="button"
              onKeyDown={(event) => {
                if (props.disabled) {
                  return
                }

                if (event.key === "ArrowDown") {
                  event.preventDefault()
                  props.onOpenChange(true)
                  return
                }

                if (event.key === "Escape" && isMenuOpen) {
                  event.preventDefault()
                  props.onOpenChange(false)
                }
              }}
            >
              <span class={cx(styles.triggerLabel, !selectedItem && styles.triggerPlaceholder)}>
                {triggerLabel}
              </span>
              <ChevronDown
                class={styles.chevron}
                size={16}
                strokeWidth={2.2}
                style={{
                  color: isMenuOpen ? token.var("colors.text") : token.var("colors.muted"),
                  transform: isMenuOpen ? "rotate(180deg)" : "rotate(0deg)",
                }}
              />
            </button>
          </Popover.Trigger>
        </span>
      </GoodTooltip>
      <MenuPortal>
        if (isMenuOpen) {
          <SessionInputSelectMenu
            filterInputRef={(element) => {
              filterInputElement = element
            }}
            filterable={isFilterable}
            items={props.items}
            loading={props.loading ?? false}
            menuRef={(element) => {
              menuElement = element
            }}
            onClose={() => {
              props.onOpenChange(false)
            }}
            onValueChange={(value) => {
              props.onValueChange(value)
              props.onOpenChange(false)
            }}
            value={props.value}
          />
        }
      </MenuPortal>
    </div>
  </Popover.Root>
}

component SessionInputSelectMenu(props: {
  filterInputRef: (element: HTMLInputElement | null) => void;
  filterable: boolean;
  items: readonly SessionInputSelectItem[];
  loading: boolean;
  menuRef: (element: HTMLDivElement | null) => void;
  onClose: () => void;
  onValueChange: (value: string) => void;
  value: string | null;
}) {
  let itemElements: Record<string, HTMLButtonElement | null> = {}

  const initialSelectedIndex = props.items.findIndex((item) => item.value === props.value)
  const [query, setQuery] = useState("")
  const [selectedIndex, setSelectedIndex] = useState(
    initialSelectedIndex >= 0 ? initialSelectedIndex : 0,
  )

  const normalizedQuery = query.trim().toLowerCase()
  const filteredItems =
    !props.filterable || normalizedQuery.length === 0
      ? props.items
      : props.items.filter((item) => {
          const searchableText = ([
            item.label,
            item.detail,
            item.searchText,
          ].filter((value) => typeof value === "string" && value.length > 0) as string[]).join(
            "\n",
          ).toLowerCase()

          return searchableText.includes(normalizedQuery)
        })
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
  }

  useEffect(() => {
    const currentIndex = filteredItems.findIndex((item) => item.value === props.value)
    setSelectedIndex(currentIndex >= 0 ? currentIndex : 0)
  }, [filteredItemSignature, props.value])

  useEffect(() => {
    if (!highlightedItem) {
      return
    }

    itemElements[highlightedItem.value]?.scrollIntoView(
      {
        block: "nearest",
      },
    )
  }, [highlightedItem?.value])

  <Popover.Positioner>
    <Popover.Content
      {ref props.menuRef}
      class={styles.menu}
      tabIndex={props.filterable ? undefined : -1}
      onKeyDown={(event) => {
        if (props.filterable) {
          return
        }

        if (event.key === "ArrowDown") {
          event.preventDefault()
          moveHighlightedIndex(1)
          return
        }

        if (event.key === "ArrowUp") {
          event.preventDefault()
          moveHighlightedIndex(-1)
          return
        }

        if (event.key === "Enter") {
          event.preventDefault()
          commitSelection(highlightedItem)
          return
        }

        if (event.key === "Escape") {
          event.preventDefault()
          props.onClose()
        }
      }}
    >
      if (props.filterable) {
        <input
          {ref props.filterInputRef}
          class={styles.menuFilter}
          placeholder="Type to filter"
          value={query}
          onInput={(event) => {
            setQuery(event.currentTarget.value)
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault()
              moveHighlightedIndex(1)
              return
            }

            if (event.key === "ArrowUp") {
              event.preventDefault()
              moveHighlightedIndex(-1)
              return
            }

            if (event.key === "Enter") {
              event.preventDefault()
              commitSelection(highlightedItem)
              return
            }

            if (event.key === "Escape") {
              event.preventDefault()
              props.onClose()
            }
          }}
        />
      }
      if (props.loading) {
        <div class={styles.menuEmpty}>
          <span
            class={css(
              {
                display: "inline-flex",
                alignItems: "center",
                gap: "8px",
              },
            )}
          >
            <LoaderCircle class={css({ animation: "spin 1s linear infinite" })} size={14} />
            {"Loading options..."}
          </span>
        </div>
      } else if (filteredItems.length === 0) {
        <div class={styles.menuEmpty}>{"No matching options."}</div>
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
                {item.label}
              </button>
            </li>
          }
        </ul>
      }
    </Popover.Content>
  </Popover.Positioner>
}
