import * as Tooltip from "@radix-ui/react-tooltip"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { Sparkles } from "lucide-react"
import { useEffect } from "preact/hooks"
import { ShellIcon } from "../support/shell-icons"
import type { NavigationItem, NavigationItemId } from "./state/Navigation"

/** Renders the icon-only primary navigation rail for the app shell. */
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
    <Tooltip.Provider delayDuration={450} skipDelayDuration={80}>
      <nav
        aria-label="Primary"
        class={css({
          display: "flex",
          flexDirection: "column",
          gap: "12px",
          width: "92px",
          padding: "18px 16px",
          borderRight: "1px solid",
          borderColor: "border",
          background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.panel")} 100%)`,
        })}
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
    </Tooltip.Provider>
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
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
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
            <ShellIcon
              name={props.item.icon}
              title={props.isFocused ? props.item.label : undefined}
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
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          class={css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            pointerEvents: "none",
            padding: "8px 12px",
            borderRadius: "12px",
            border: "1px solid",
            borderColor: "border",
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
            color: "text",
            fontSize: "0.8rem",
            fontWeight: "620",
            lineHeight: "1",
            boxShadow: "0 18px 40px rgba(121, 138, 160, 0.16)",
            transformOrigin: "var(--radix-tooltip-content-transform-origin)",
            transition:
              "opacity 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)",
            zIndex: "10",
            "@starting-style": {
              opacity: "0",
              transform: "scale(0.97)",
            },
          })}
          side="right"
          sideOffset={12}
        >
          {props.item.label}
          <Tooltip.Arrow
            class={css({
              fill: token.var("colors.background"),
              stroke: token.var("colors.border"),
              strokeWidth: "1",
            })}
            height={6}
            width={10}
          />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
