/** Shared custom dropdown used by the launch-session selectors. */
import { Popover } from "@ark-ui/react/popover"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { ChevronDown, LoaderCircle } from "lucide-react"
import { useEffect, useRef, useState } from "preact/hooks"

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
  const menuRef = useRef<HTMLDivElement | null>(null)
  const filterInputRef = useRef<HTMLInputElement | null>(null)
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const itemRefs = useRef<Record<string, HTMLButtonElement | null>>({})

  const [query, setQuery] = useState("")
  const [selectedIndex, setSelectedIndex] = useState(0)

  const isMenuOpen = props.open
  const isFilterable = props.filterable ?? false
  const selectedItem = props.items.find((item) => item.value === props.value) ?? null
  const normalizedQuery = query.trim().toLowerCase()
  const filteredItems =
    !isFilterable || normalizedQuery.length === 0
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
  const shortcutLabel = props.shortcut ? `${props.shortcut} opens menu` : "No shortcut assigned"
  const triggerLabel = selectedItem?.label ?? props.placeholder
  const ariaLabel = `${props.label}: ${triggerLabel}`

  useEffect(() => {
    if (!isMenuOpen) {
      setQuery("")
      setSelectedIndex(0)
      return
    }

    const currentIndex = filteredItems.findIndex((item) => item.value === props.value)
    setSelectedIndex(currentIndex >= 0 ? currentIndex : 0)
  }, [filteredItemSignature, isFilterable, isMenuOpen, props.value])

  useEffect(() => {
    if (!isMenuOpen || !highlightedItem) {
      return
    }

    itemRefs.current[highlightedItem.value]?.scrollIntoView(
      {
        block: "nearest",
      },
    )
  }, [highlightedItem?.value, isMenuOpen])

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
    props.onOpenChange(false)
  }

  <Popover.Root
    autoFocus={true}
    closeOnInteractOutside={true}
    finalFocusEl={() => triggerRef.current}
    immediate={true}
    initialFocusEl={() => isFilterable ? filterInputRef.current : menuRef.current}
    lazyMount={true}
    modal={isFilterable}
    open={isMenuOpen}
    positioning={{
      fitViewport: true,
      getAnchorElement: () => triggerRef.current,
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
        content={<tsx:react>
          <span class={styles.tooltipContent}>
            <span>
              {props.label}
            </span>
            <span class={styles.tooltipShortcut}>
              {shortcutLabel}
            </span>
          </span>
        </tsx:react>}
      >
        <span class={styles.tooltipTrigger}>
          <Popover.Trigger asChild>
            <button
              ref={triggerRef}
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
        <Popover.Positioner>
          <Popover.Content
            ref={menuRef}
            class={styles.menu}
            tabIndex={isFilterable ? undefined : -1}
            onKeyDown={(event) => {
              if (isFilterable) {
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
                props.onOpenChange(false)
              }
            }}
          >
            {isFilterable
              ? <tsx:react>
                  <input ref={filterInputRef} class={styles.menuFilter} placeholder="Type to filter" value={query} onInput={(
                    event,
                  ) => {
                    setQuery(event.currentTarget.value)
                  }} onKeyDown={(event) => {
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
                      props.onOpenChange(false)
                    }
                  }} />
                </tsx:react>
              : null}
            {props.loading
              ? <tsx:react>
                  <div class={styles.menuEmpty}>
                    <span class={css(
                      {
                        display: "inline-flex",
                        alignItems: "center",
                        gap: "8px",
                      },
                    )}>
                      <LoaderCircle class={css(
                        { animation: "spin 1s linear infinite" },
                      )} size={14} />
                      Loading options...
                    </span>
                  </div>
                </tsx:react>
              : filteredItems.length === 0
                ? <tsx:react>
                    <div class={styles.menuEmpty}>No matching options.</div>
                  </tsx:react>
                : <tsx:react>
                    <ul class={styles.menuList}>
                      {filteredItems.map((item, index) => {
                        const isActive = index === selectedIndex

                        return <tsx:react>
                          <li key={item.value}>
                            <button ref={(element) => {
                              itemRefs.current[item.value] = element
                            }} class={cx(
                              styles.menuButton,
                              isActive && styles.menuButtonActive,
                            )} disabled={item.disabled} type="button" onMouseEnter={() => {
                              setSelectedIndex(index)
                            }} onClick={() => {
                              commitSelection(item)
                            }}>
                              {item.label}
                            </button>
                          </li>
                        </tsx:react>
                      })}
                    </ul>
                  </tsx:react>}
          </Popover.Content>
        </Popover.Positioner>
      </MenuPortal>
    </div>
  </Popover.Root>
}
