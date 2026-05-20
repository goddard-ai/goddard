import { type Signal } from "@preact/signals"
import { createContext } from "preact"
import { useContext, useRef } from "preact/hooks"

import { type FloatingReference } from "./overlay/position.ts"
import { Popover } from "./popover.tsrx"

type MenuContextValue = {
    close: () => void;
  }

export type MenuProps = {
    anchor: () => FloatingReference | null;
    ariaLabel: string;
    class?: string;
    children?: preact.ComponentChildren;
    open: Signal<boolean>;
  }

export type MenuItemProps = {
    children?: preact.ComponentChildren;
    class?: string;
    disabled?: boolean;
    onSelect?: () => void;
  }

const menuContext = createContext<MenuContextValue | null>(null)

export component Menu(props: MenuProps) {
  const menuRef = useRef<HTMLDivElement | null>(null)

  const close = () => {
    props.open.value = false
  }

  <menuContext.Provider value={{ close }}>
    <Popover
      anchor={props.anchor}
      ariaLabel={props.ariaLabel}
      class={props.class}
      closeOnEscape={true}
      closeOnOutsidePointer={true}
      initialFocus={() => menuRef.current?.querySelector<HTMLElement>(
        "[role=\"menuitem\"]:not([aria-disabled=\"true\"])",
      ) ??
        null}
      offset={4}
      open={props.open}
      placement="bottom-start"
      restoreFocus={true}
      role="menu"
      strategy="fixed"
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
      <div ref={menuRef}>
        {props.children}
      </div>
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

export component MenuItem(props: MenuItemProps) {
  const menu = useContext(menuContext)

  <button
    aria-disabled={props.disabled ? "true" : undefined}
    class={props.class}
    role="menuitem"
    tabIndex={props.disabled ? undefined : -1}
    type="button"
    onClick={() => {
      if (props.disabled) {
        return
      }

      props.onSelect?.()
      menu?.close()
    }}
    onMouseEnter={(event) => {
      if (!props.disabled) {
        event.currentTarget.focus()
      }
    }}
  >
    {props.children}
  </button>
}
