/** Shared custom dropdown used by the launch-session selectors. */
import { css, cx } from "@goddard-ai/styled-system/css"
import { Check, ChevronDown, Circle, LoaderCircle } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "preact/hooks"

import { MenuPortal } from "~/lib/menu-portal.tsx"
import {
  inputMenuBodyClass,
  inputMenuButtonActiveClass,
  inputMenuButtonClass,
  inputMenuClass,
  inputMenuDetailClass,
  inputMenuEmptyClass,
  inputMenuFilterClass,
  inputMenuHeaderClass,
  inputMenuIconClass,
  inputMenuLabelClass,
  inputMenuListClass,
} from "./menu-styles.ts"

const fieldClass = css({
  display: "grid",
  gap: "6px",
})

const labelClass = css({
  color: "text",
  fontSize: "0.84rem",
  fontWeight: "600",
})

const triggerClass = css({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  alignItems: "center",
  gap: "10px",
  width: "100%",
  minHeight: "42px",
  padding: "10px 12px",
  borderRadius: "14px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "text",
  cursor: "pointer",
  textAlign: "left",
  outline: "none",
  transition:
    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    borderColor: "accentStrong",
    boxShadow: "0 0 0 3px color-mix(in srgb, var(--colors-accent) 16%, transparent)",
  },
  _disabled: {
    cursor: "not-allowed",
    opacity: "0.56",
  },
})

const triggerLabelClass = css({
  minWidth: "0",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontSize: "0.9rem",
  fontWeight: "600",
})

const triggerPlaceholderClass = css({
  color: "muted",
  fontWeight: "500",
})

/** One item rendered inside a launch-session selector menu. */
export type SessionInputSelectItem = {
  value: string
  label: string
  detail?: string | null
  searchText?: string | null
  icon?: preact.FunctionComponent<{ size?: number; strokeWidth?: number }>
  disabled?: boolean
}

/** Props for one shared launch-session selector menu. */
export type SessionInputSelectProps = {
  label: string
  placeholder: string
  value: string | null
  items: readonly SessionInputSelectItem[]
  open: boolean
  disabled?: boolean
  filterable?: boolean
  loading?: boolean
  menuLabel?: string
  onOpenChange: (open: boolean) => void
  onValueChange: (value: string) => void
}

/** Returns one viewport-safe fixed menu position under the trigger. */
function getMenuPosition(trigger: HTMLButtonElement | null) {
  const rect = trigger?.getBoundingClientRect()

  if (!rect) {
    return {
      left: 16,
      top: 16,
      width: 320,
    }
  }

  const width = Math.min(Math.max(rect.width, 280), window.innerWidth - 32)
  const left = Math.max(16, Math.min(rect.left, window.innerWidth - width - 16))

  return {
    left,
    top: Math.max(16, Math.min(rect.bottom + 10, window.innerHeight - 32)),
    width,
  }
}

/** Shared dropdown control used for project, adapter, branch, model, and thinking selectors. */
export function SessionInputSelect(props: SessionInputSelectProps) {
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const filterInputRef = useRef<HTMLInputElement | null>(null)
  const [query, setQuery] = useState("")
  const [selectedIndex, setSelectedIndex] = useState(0)

  const selectedItem = props.items.find((item) => item.value === props.value) ?? null
  const filteredItems = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase()

    if (!props.filterable || normalizedQuery.length === 0) {
      return props.items
    }

    return props.items.filter((item) => {
      const searchableText = [item.label, item.detail, item.searchText]
        .filter((value): value is string => typeof value === "string" && value.length > 0)
        .join("\n")
        .toLowerCase()

      return searchableText.includes(normalizedQuery)
    })
  }, [props.filterable, props.items, query])

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
  }, [filteredItems, props.filterable, props.open, props.value])

  useEffect(() => {
    if (!props.open) {
      return
    }

    function handlePointerDown(event: MouseEvent) {
      const target = event.target

      if (!(target instanceof Node)) {
        return
      }

      if (menuRef.current?.contains(target) || triggerRef.current?.contains(target)) {
        return
      }

      props.onOpenChange(false)
    }

    function handleViewportChange() {
      props.onOpenChange(false)
    }

    document.addEventListener("mousedown", handlePointerDown)
    window.addEventListener("resize", handleViewportChange)
    window.addEventListener("scroll", handleViewportChange, true)

    return () => {
      document.removeEventListener("mousedown", handlePointerDown)
      window.removeEventListener("resize", handleViewportChange)
      window.removeEventListener("scroll", handleViewportChange, true)
    }
  }, [props.onOpenChange, props.open])

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

  const menuPosition = props.open ? getMenuPosition(triggerRef.current) : null
  const highlightedItem = filteredItems[selectedIndex] ?? filteredItems[0] ?? null

  return (
    <label class={fieldClass}>
      <span class={labelClass}>{props.label}</span>
      <button
        ref={triggerRef}
        class={triggerClass}
        disabled={props.disabled}
        type="button"
        onClick={() => {
          if (props.disabled) {
            return
          }

          props.onOpenChange(!props.open)
        }}
        onKeyDown={(event) => {
          if (props.disabled) {
            return
          }

          if (
            event.key === "ArrowDown" ||
            event.key === "Enter" ||
            event.key === " " ||
            event.key === "Spacebar"
          ) {
            event.preventDefault()
            props.onOpenChange(true)
          }

          if (event.key === "Escape" && props.open) {
            event.preventDefault()
            props.onOpenChange(false)
          }
        }}
      >
        <span class={cx(triggerLabelClass, !selectedItem && triggerPlaceholderClass)}>
          {selectedItem?.label ?? props.placeholder}
        </span>
        <ChevronDown
          class={css({
            color: props.open ? "text" : "muted",
            transition: "transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
            transform: props.open ? "rotate(180deg)" : "rotate(0deg)",
          })}
          size={16}
          strokeWidth={2.2}
        />
      </button>

      {props.open && menuPosition ? (
        <MenuPortal>
          <div
            ref={menuRef}
            class={inputMenuClass}
            style={{
              left: `${menuPosition.left}px`,
              top: `${menuPosition.top}px`,
              width: `${menuPosition.width}px`,
            }}
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
            <div class={inputMenuHeaderClass}>{props.menuLabel ?? props.label}</div>
            {props.filterable ? (
              <input
                ref={filterInputRef}
                class={inputMenuFilterClass}
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
            <div class={inputMenuListClass}>
              {props.loading ? (
                <div class={inputMenuEmptyClass}>
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
                <div class={inputMenuEmptyClass}>No matching options.</div>
              ) : (
                filteredItems.map((item, index) => {
                  const Icon = item.icon
                  const isActive = index === selectedIndex

                  return (
                    <button
                      key={item.value}
                      class={cx(inputMenuButtonClass, isActive && inputMenuButtonActiveClass)}
                      disabled={item.disabled}
                      type="button"
                      onMouseDown={(event) => {
                        event.preventDefault()
                      }}
                      onMouseEnter={() => {
                        setSelectedIndex(index)
                      }}
                      onClick={() => {
                        commitSelection(item)
                      }}
                    >
                      <span class={inputMenuIconClass} aria-hidden="true">
                        {props.value === item.value ? (
                          <Check size={14} strokeWidth={2.4} />
                        ) : Icon ? (
                          <Icon size={14} strokeWidth={2.2} />
                        ) : (
                          <Circle size={12} strokeWidth={2} />
                        )}
                      </span>
                      <span class={inputMenuBodyClass}>
                        <span class={inputMenuLabelClass}>{item.label}</span>
                        {item.detail ? (
                          <span class={inputMenuDetailClass}>{item.detail}</span>
                        ) : null}
                      </span>
                    </button>
                  )
                })
              )}
            </div>
          </div>
        </MenuPortal>
      ) : null}
    </label>
  )
}
