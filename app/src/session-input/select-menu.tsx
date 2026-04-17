/** Shared custom dropdown used by the launch-session selectors. */
import { Popover } from "@ark-ui/react/popover"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { ChevronDown, LoaderCircle } from "lucide-react"
import { useEffect, useRef, useState } from "preact/hooks"

import { GoodTooltip } from "~/lib/good-tooltip.tsx"
import styles from "./select-menu.style.ts"

/** One item rendered inside a launch-session selector menu. */
export type SessionInputSelectItem = {
  value: string
  label: string
  detail?: string | null
  searchText?: string | null
  disabled?: boolean
}

/** Props for one shared launch-session selector menu. */
export type SessionInputSelectProps = {
  label: string
  placeholder: string
  shortcut?: string | null
  value: string | null
  items: readonly SessionInputSelectItem[]
  open: boolean
  disabled?: boolean
  filterable?: boolean
  loading?: boolean
  onOpenChange: (open: boolean) => void
  onValueChange: (value: string) => void
}

/** Shared dropdown control used for project, adapter, branch, model, and thinking selectors. */
export function SessionInputSelect(props: SessionInputSelectProps) {
  const menuRef = useRef<HTMLDivElement | null>(null)
  const filterInputRef = useRef<HTMLInputElement | null>(null)
  const itemRefs = useRef<Record<string, HTMLButtonElement | null>>({})
  const [query, setQuery] = useState("")
  const [selectedIndex, setSelectedIndex] = useState(0)

  const selectedItem = props.items.find((item) => item.value === props.value) ?? null
  const normalizedQuery = query.trim().toLowerCase()
  const filteredItems =
    !props.filterable || normalizedQuery.length === 0
      ? props.items
      : props.items.filter((item) => {
          const searchableText = [item.label, item.detail, item.searchText]
            .filter((value): value is string => typeof value === "string" && value.length > 0)
            .join("\n")
            .toLowerCase()

          return searchableText.includes(normalizedQuery)
        })
  const filteredItemSignature = filteredItems.map((item) => item.value).join("\n")
  const highlightedItem = filteredItems[selectedIndex] ?? filteredItems[0] ?? null

  useEffect(() => {
    if (!props.open) {
      setQuery("")
      setSelectedIndex(0)
      return
    }

    const currentIndex = filteredItems.findIndex((item) => item.value === props.value)
    setSelectedIndex(currentIndex >= 0 ? currentIndex : 0)

    queueMicrotask(() => {
      if (props.filterable) {
        const input = filterInputRef.current
        input?.focus()
        input?.setSelectionRange(input.value.length, input.value.length)
        return
      }

      menuRef.current?.focus()
    })
  }, [filteredItemSignature, props.filterable, props.open, props.value])

  useEffect(() => {
    if (!props.open || !highlightedItem) {
      return
    }

    itemRefs.current[highlightedItem.value]?.scrollIntoView({
      block: "nearest",
    })
  }, [highlightedItem?.value, props.open])

  function highlightNext(step: 1 | -1) {
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

  const shortcutLabel = props.shortcut ? `${props.shortcut} opens menu` : "No shortcut assigned"
  const triggerLabel = selectedItem?.label ?? props.placeholder
  const ariaLabel = `${props.label}: ${triggerLabel}`

  return (
    <Popover.Root
      closeOnInteractOutside={true}
      initialFocusEl={() => (props.filterable ? filterInputRef.current : menuRef.current)}
      lazyMount={true}
      open={props.open}
      portalled={false}
      positioning={{
        gutter: 8,
        placement: "bottom-start",
        sameWidth: true,
      }}
      unmountOnExit={true}
      onOpenChange={(details) => {
        props.onOpenChange(details.open)
      }}
    >
      <div class={styles.field}>
        <GoodTooltip
          ariaLabel={props.label}
          content={
            <span class={styles.tooltipContent}>
              <span>{props.label}</span>
              <span class={styles.tooltipShortcut}>{shortcutLabel}</span>
            </span>
          }
        >
          <span class={styles.tooltipTrigger}>
            <Popover.Trigger asChild>
              <button
                aria-expanded={props.open}
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

                  if (event.key === "Escape" && props.open) {
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
                    color: props.open ? token.var("colors.text") : token.var("colors.muted"),
                    transform: props.open ? "rotate(180deg)" : "rotate(0deg)",
                  }}
                />
              </button>
            </Popover.Trigger>
          </span>
        </GoodTooltip>

        <Popover.Positioner>
          <Popover.Content
            ref={menuRef}
            class={styles.menu}
            tabIndex={props.filterable ? undefined : -1}
            onKeyDown={(event) => {
              if (props.filterable) {
                return
              }

              if (event.key === "ArrowDown") {
                event.preventDefault()
                highlightNext(1)
                return
              }

              if (event.key === "ArrowUp") {
                event.preventDefault()
                highlightNext(-1)
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
            {props.filterable ? (
              <input
                ref={filterInputRef}
                class={styles.menuFilter}
                placeholder="Type to filter"
                value={query}
                onInput={(event) => {
                  setQuery(event.currentTarget.value)
                }}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown") {
                    event.preventDefault()
                    highlightNext(1)
                    return
                  }

                  if (event.key === "ArrowUp") {
                    event.preventDefault()
                    highlightNext(-1)
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
              />
            ) : null}

            {props.loading ? (
              <div class={styles.menuEmpty}>
                <span
                  class={css({
                    display: "inline-flex",
                    alignItems: "center",
                    gap: "8px",
                  })}
                >
                  <LoaderCircle class={css({ animation: "spin 1s linear infinite" })} size={14} />
                  Loading options...
                </span>
              </div>
            ) : filteredItems.length === 0 ? (
              <div class={styles.menuEmpty}>No matching options.</div>
            ) : (
              <ul class={styles.menuList}>
                {filteredItems.map((item, index) => {
                  const isActive = index === selectedIndex

                  return (
                    <li key={item.value}>
                      <button
                        ref={(element) => {
                          itemRefs.current[item.value] = element
                        }}
                        class={cx(styles.menuButton, isActive && styles.menuButtonActive)}
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
                  )
                })}
              </ul>
            )}
          </Popover.Content>
        </Popover.Positioner>
      </div>
    </Popover.Root>
  )
}
