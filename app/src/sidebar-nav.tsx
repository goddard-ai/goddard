import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { Sparkles } from "lucide-react"
import { useEffect } from "preact/hooks"
import { GoodTooltip } from "./support/good-tooltip.tsx"
import { SvgIcon } from "./svg-icon.tsx"
import type { NavigationItem, NavigationItemId } from "./navigation.ts"
import { getWorkbenchTabIcon } from "./workbench-tab-registry.ts"

/** Renders the icon-only primary navigation rail for the app shell. */
export function SidebarNav(props: {
  class?: string
  items: Array<NavigationItem & { badgeCount?: number }>
  selectedItemId: NavigationItemId
  onSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
}) {
  const focusedIndex = useSignal(props.items.findIndex((item) => item.id === props.selectedItemId))

  useEffect(() => {
    focusedIndex.value = props.items.findIndex((item) => item.id === props.selectedItemId)
  }, [focusedIndex, props.items, props.selectedItemId])

  return (
    <nav
      aria-label="Primary"
      class={[
        css({
          display: "flex",
          flexDirection: "column",
          gap: "12px",
          width: "92px",
          padding: "18px 16px",
          borderRight: "1px solid",
          borderColor: "border",
          background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.panel")} 100%)`,
        }),
        props.class,
      ].join(" ")}
    >
      <div
        class={css({
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: "8px",
          marginBottom: "8px",
        })}
      >
        <div
          class={css({
            display: "grid",
            placeItems: "center",
            width: "56px",
            height: "56px",
            borderRadius: "20px",
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
            boxShadow: `0 16px 32px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), inset 0 0 0 1px ${token.var("colors.border")}`,
          })}
        >
          <Sparkles size={18} strokeWidth={1.9} />
        </div>
        <div
          class={css({
            color: "accentStrong",
            fontSize: "0.7rem",
            fontWeight: "700",
            letterSpacing: "0.18em",
            textTransform: "uppercase",
          })}
        >
          G
        </div>
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
  onSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
}) {
  return (
    <GoodTooltip
      ariaLabel={props.item.ariaLabel}
      content={props.item.label}
      side="right"
      sideOffset={12}
    >
      <button
        aria-current={props.isSelected ? "page" : undefined}
        aria-label={props.item.ariaLabel}
        class={css({
          position: "relative",
          display: "grid",
          placeItems: "center",
          width: "100%",
          height: "54px",
          borderRadius: "18px",
          border: "none",
          backgroundColor: "transparent",
          color: "muted",
          cursor: "pointer",
          transition:
            "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          _active: {
            transform: "scale(0.97)",
          },
          "@media (hover: hover) and (pointer: fine)": {
            _hover: {
              backgroundColor: "background",
              color: "text",
              boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
            },
          },
          _focusVisible: {
            outline: `2px solid ${token.var("colors.accentStrong")}`,
            outlineOffset: "2px",
          },
          "&[data-selected='true']": {
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
            color: "accentStrong",
            boxShadow: `0 12px 30px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent), inset 0 0 0 1px ${token.var("colors.border")}`,
          },
        })}
        data-selected={props.isSelected}
        type="button"
        onClick={(event) => {
          props.onSelect(props.item.id, { openInTab: event.metaKey })
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
          <SvgIcon
            aria-hidden={props.isFocused ? undefined : true}
            aria-label={props.isFocused ? props.item.label : undefined}
            name={getWorkbenchTabIcon(props.item.id)}
            height="20px"
            width="20px"
          />
        </span>
        {props.item.badgeCount ? (
          <span
            class={css({
              position: "absolute",
              top: "7px",
              right: "7px",
              minWidth: "20px",
              height: "20px",
              paddingInline: "6px",
              borderRadius: "999px",
              backgroundColor: "accentStrong",
              color: "background",
              boxShadow: `0 8px 20px color-mix(in srgb, ${token.var("colors.accent")} 24%, transparent)`,
              fontSize: "11px",
              fontWeight: "700",
              lineHeight: "20px",
              textAlign: "center",
            })}
          >
            {props.item.badgeCount}
          </span>
        ) : null}
      </button>
    </GoodTooltip>
  )
}
