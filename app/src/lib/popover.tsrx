import { type Signal } from "@preact/signals"
import { useId, useLayoutEffect, useRef } from "preact/hooks"

import { startOverlayDismissal } from "./overlay/dismissal.ts"
import { focusFirstElement, keepFocusInside, restoreFocus } from "./overlay/focus.ts"
import { OverlayPortal } from "./overlay/portal.tsrx"
import { startFloatingPosition, type FloatingPositionOptions } from "./overlay/position.ts"
import { overlayStack } from "./overlay/stack.ts"

export type PopoverCloseReason = "anchor-lost" | "escape" | "outside"

export type PopoverProps = FloatingPositionOptions & {
    open: boolean | Signal<boolean>;
    anchor: () => HTMLElement | null;
    class?: string;
    closeOnEscape?: boolean;
    closeOnOutsidePointer?: boolean;
    modalFocus?: boolean;
    initialFocus?: () => HTMLElement | null;
    restoreFocus?: boolean;
    onKeyDown?: (event: preact.TargetedKeyboardEvent<HTMLDivElement>) => void;
    onOpenChange?: (open: boolean, reason: PopoverCloseReason) => void;
    children?: preact.ComponentChildren;
  }

function readOpen(open: boolean | Signal<boolean>) {
  return typeof open === "boolean" ? open : open.value
}

function writeOpen(open: boolean | Signal<boolean>, nextOpen: boolean) {
  if (typeof open !== "boolean") {
    open.value = nextOpen
  }
}

/** Renders unstyled anchored overlay content for feature-owned popovers. */
export component Popover(props: PopoverProps) {
  const popoverId = useId()
  const contentRef = useRef<HTMLDivElement | null>(null)
  const isOpen = readOpen(props.open)

  function requestClose(reason: PopoverCloseReason) {
    writeOpen(props.open, false)
    props.onOpenChange?.(false, reason)
  }

  useLayoutEffect(() => {
    if (!isOpen) {
      return
    }

    const anchorElement = props.anchor()
    const contentElement = contentRef.current

    if (!anchorElement || !contentElement) {
      requestClose("anchor-lost")
      return
    }

    const previouslyFocusedElement = document.activeElement
    const unregisterOverlay = overlayStack.register({
      id: popoverId,
      elements: [anchorElement, contentElement],
    })
    const stopDismissal = startOverlayDismissal({
      id: popoverId,
      dismissOnEscape: props.closeOnEscape ?? true,
      dismissOnOutsidePointer: props.closeOnOutsidePointer ?? true,
      close(reason) {
        requestClose(reason)
      },
    })
    const stopPositioning = startFloatingPosition(anchorElement, contentElement, {
      offset: props.offset,
      placement: props.placement,
      sameWidth: props.sameWidth,
      strategy: props.strategy,
    })
    const anchorObserver = new MutationObserver(() => {
      if (!document.contains(anchorElement)) {
        requestClose("anchor-lost")
      }
    })

    anchorObserver.observe(document, { childList: true, subtree: true })

    const initialFocusElement = props.initialFocus?.()
    if (initialFocusElement) {
      initialFocusElement.focus()
    } else if (!focusFirstElement(contentElement)) {
      contentElement.focus()
    }

    return () => {
      anchorObserver.disconnect()
      stopPositioning()
      stopDismissal()
      unregisterOverlay()

      if (props.restoreFocus ?? true) {
        restoreFocus(anchorElement)
      } else {
        restoreFocus(previouslyFocusedElement)
      }
    }
  }, [isOpen, popoverId])

  if (!isOpen) {
    return
  }

  <OverlayPortal root="menu">
    <div
      ref={contentRef}
      class={props.class}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (props.modalFocus) {
          keepFocusInside(event.currentTarget, event)
        }

        props.onKeyDown?.(event)
      }}
    >
      {props.children}
    </div>
  </OverlayPortal>
}
