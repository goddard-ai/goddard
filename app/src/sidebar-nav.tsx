import { useSignal } from "@preact/signals"
import { Sparkles } from "lucide-react"
import { useEffect } from "preact/hooks"

import { GoodIcon } from "./lib/good-icon.tsx"
import { GoodTooltip } from "./lib/good-tooltip.tsx"
import type { NavigationItem, NavigationItemId } from "./navigation.ts"
import styles from "./sidebar-nav.style.ts"
import { getWorkbenchTabIcon } from "./workbench-tab-registry.ts"

/** Renders the icon-only primary navigation rail for the app shell. */
export function SidebarNav(props: {
  class?: string
  items: NavigationItem[]
  selectedItemId: NavigationItemId
  onSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
}) {
  const focusedIndex = useSignal(props.items.findIndex((item) => item.id === props.selectedItemId))

  useEffect(() => {
    focusedIndex.value = props.items.findIndex((item) => item.id === props.selectedItemId)
  }, [focusedIndex, props.items, props.selectedItemId])

  return (
    <nav aria-label="Primary" class={[styles.root, props.class].filter(Boolean).join(" ")}>
      <div class={styles.brand}>
        <div class={styles.mark}>
          <Sparkles size={18} strokeWidth={1.9} />
        </div>
        <div class={styles.markLabel}>G</div>
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
  item: NavigationItem
  isSelected: boolean
  isFocused: boolean
  onFocus: () => void
  onMoveFocus: (delta: number) => void
  onSelect: (id: NavigationItemId, options?: { openInTab?: boolean }) => void
}) {
  return (
    <GoodTooltip
      ariaLabel={props.item.label}
      content={props.item.label}
      side="right"
      sideOffset={12}
    >
      <button
        aria-current={props.isSelected ? "page" : undefined}
        aria-label={props.item.label}
        class={styles.item}
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
        <span class={styles.icon}>
          <GoodIcon
            aria-hidden={props.isFocused ? undefined : true}
            aria-label={props.isFocused ? props.item.label : undefined}
            name={getWorkbenchTabIcon(props.item.id)}
            height="20px"
            width="20px"
          />
        </span>
      </button>
    </GoodTooltip>
  )
}
