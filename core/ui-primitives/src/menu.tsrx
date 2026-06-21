import { type Signal } from "@preact/signals"
import { createContext } from "preact"
import { useContext, useMemo, useRef } from "preact/hooks"

import { type FloatingReference } from "./overlay/position.ts"
import { Popover } from "./popover.tsrx"

type MenuControls = {
  close: () => void;
}

export type MenuProps = {
  anchor: () => FloatingReference | null;
  ariaLabel?: string;
  ariaLabelledBy?: string;
  class?: string;
  children?: preact.ComponentChildren;
  id?: string;
  open: Signal<boolean>;
  style?: preact.CSSProperties;
}

export type MenuItemProps = {
  children?: preact.ComponentChildren;
  class?: string;
  disabled?: boolean;
  id?: string;
  onSelect?: () => void;
  style?: preact.CSSProperties;
}

const menuContext = createContext<MenuControls | null>(null)

export function Menu(props: MenuProps) @{
  const menuRef = useRef<HTMLDivElement | null>(null)

  const menuControls = useMemo<MenuControls>(
    () => ({
      close() {
        props.open.value = false
      },
    }),
    [props.open],
  )

  <menuContext.Provider value={menuControls}>
    <Popover
      anchor={props.anchor}
      ariaLabel={props.ariaLabel}
      ariaLabelledBy={props.ariaLabelledBy}
      class={props.class}
      closeOnEscape={true}
      closeOnOutsidePointer={true}
      id={props.id}
      initialFocus={() =>
        menuRef.current?.querySelector<HTMLElement>(
          "[role=\"menuitem\"]:not([aria-disabled=\"true\"])",
        ) ?? null}
      offset={4}
      open={props.open}
      placement="bottom-start"
      restoreFocus={true}
      role="menu"
      strategy="fixed"
      style={props.style}
      onKeyDown={(event) => {
        const menu = menuRef.current

        if (!menu) {
          return
        }

        const items = getMenuItems(menu)
        const currentIndex = items.findIndex((item) => item === document.activeElement)

        switch (event.key) {
          case "ArrowDown":
            event.preventDefault()
            focusMenuItem(menu, currentIndex + 1)
            return
          case "ArrowUp":
            event.preventDefault()
            focusMenuItem(menu, currentIndex - 1)
            return
          case "Home":
            event.preventDefault()
            focusMenuItem(menu, 0)
            return
          case "End":
            event.preventDefault()
            focusMenuItem(menu, items.length - 1)
            return
          case "Enter":
          case " ":
            if (document.activeElement instanceof HTMLElement) {
              event.preventDefault()
              document.activeElement.click()
            }
        }
      }}
    >
      <div ref={menuRef}>{props.children}</div>
    </Popover>
  </menuContext.Provider>
}

function getMenuItems(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>("[role=\"menuitem\"]")).filter(
    (item) => item.getAttribute("aria-disabled") !== "true",
  )
}

function focusMenuItem(container: HTMLElement, index: number) {
  const items = getMenuItems(container)

  if (items.length === 0) {
    return
  }

  const wrappedIndex = (index + items.length) % items.length
  const item = items[wrappedIndex]

  if (item) {
    item.focus()
  }
}

export function MenuItem(props: MenuItemProps) @{
  const menu = useContext(menuContext)

  <button
    aria-disabled={props.disabled ? "true" : undefined}
    class={props.class}
    id={props.id}
    role="menuitem"
    style={props.style}
    tabIndex={props.disabled ? undefined : -1}
    type="button"
    onClick={() => {
      if (props.disabled) {
        return
      }

      props.onSelect?.()
      menu?.close()
    }}
  >{props.children}</button>
}
