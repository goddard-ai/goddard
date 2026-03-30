import { useSignal } from "@preact/signals"
import { useEffect } from "preact/hooks"
import { css } from "../../styled-system/css"
import { token } from "../../styled-system/tokens"
import type { NavigationItem, NavigationItemId } from "../state/navigation-state"
import { ShellIcon } from "../support/shell-icons"

/** Renders the icon-only primary navigation rail for the sprint-1 shell. */
export function SidebarNav(props: {
  items: Array<NavigationItem & { badgeCount?: number }>
  selectedItemId: NavigationItemId
  onSelect: (id: NavigationItemId) => void
}) {
  const focusedIndex = useSignal(props.items.findIndex((item) => item.id === props.selectedItemId))

  useEffect(() => {
    focusedIndex.value = props.items.findIndex((item) => item.id === props.selectedItemId)
  }, [focusedIndex, props.items, props.selectedItemId])

  return (
    <nav
      aria-label="Primary"
      class={css({
        display: "flex",
        flexDirection: "column",
        gap: "10px",
        width: "84px",
        padding: "18px 14px",
        borderRight: "1px solid",
        borderColor: "border",
        backgroundColor: "surface",
      })}
    >
      <div
        class={css({
          display: "grid",
          placeItems: "center",
          height: "52px",
          marginBottom: "10px",
          borderRadius: "16px",
          backgroundColor: "panel",
          color: "accentStrong",
          fontSize: "1.1rem",
          fontWeight: "700",
          letterSpacing: "0.08em",
        })}
      >
        G
      </div>
      {props.items.map((item, index) => (
        <SidebarNavItem
          key={item.id}
          isFocused={focusedIndex.value === index}
          isSelected={item.id === props.selectedItemId}
          item={item}
          onFocus={() => {
            focusedIndex.value = index
          }}
          onMoveFocus={(delta) => {
            focusedIndex.value = (index + delta + props.items.length) % props.items.length
          }}
          onSelect={props.onSelect}
        />
      ))}
    </nav>
  )
}

/** Renders one accessible icon button inside the left navigation rail. */
function SidebarNavItem(props: {
  item: NavigationItem & { badgeCount?: number }
  isSelected: boolean
  isFocused: boolean
  onFocus: () => void
  onMoveFocus: (delta: number) => void
  onSelect: (id: NavigationItemId) => void
}) {
  return (
    <button
      aria-current={props.isSelected ? "page" : undefined}
      aria-label={props.item.ariaLabel}
      class={css({
        position: "relative",
        display: "grid",
        placeItems: "center",
        width: "100%",
        height: "52px",
        borderRadius: "16px",
        border: "none",
        backgroundColor: "transparent",
        color: "muted",
        cursor: "pointer",
        transition: "background-color 140ms ease, color 140ms ease, transform 140ms ease",
        _hover: {
          backgroundColor: "panel",
          color: "text",
        },
        _focusVisible: {
          outline: `2px solid ${token.var("colors.accentStrong")}`,
          outlineOffset: "2px",
        },
        "&[data-selected='true']": {
          backgroundColor: "background",
          color: "accentStrong",
          boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
        },
      })}
      data-selected={props.isSelected}
      title={props.item.label}
      type="button"
      onClick={() => {
        props.onSelect(props.item.id)
      }}
      onFocus={props.onFocus}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown") {
          event.preventDefault()
          props.onMoveFocus(1)
        }

        if (event.key === "ArrowUp") {
          event.preventDefault()
          props.onMoveFocus(-1)
        }
      }}
    >
      <span class={css({ width: "20px", height: "20px" })}>
        <ShellIcon name={props.item.icon} title={props.isFocused ? props.item.label : undefined} />
      </span>
      {props.item.badgeCount ? (
        <span
          class={css({
            position: "absolute",
            top: "8px",
            right: "8px",
            minWidth: "18px",
            height: "18px",
            paddingInline: "5px",
            borderRadius: "999px",
            backgroundColor: "accentStrong",
            color: "background",
            fontSize: "11px",
            fontWeight: "700",
            lineHeight: "18px",
            textAlign: "center",
          })}
        >
          {props.item.badgeCount}
        </span>
      ) : null}
    </button>
  )
}
