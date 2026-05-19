import { type Signal } from "@preact/signals"
import { useId, useLayoutEffect, useRef } from "preact/hooks"

import { startOverlayDismissal } from "./overlay/dismissal.ts"
import { focusFirstElement, keepFocusInside, restoreFocus } from "./overlay/focus.ts"
import { OverlayPortal } from "./overlay/portal.tsrx"
import {
  startFloatingPosition,
  type FloatingPositionOptions,
  type FloatingReference,
} from "./overlay/position.ts"
import { overlayStack } from "./overlay/stack.ts"

export type PopoverCloseReason = "anchor-lost" | "escape" | "outside"

export type PopoverProps = FloatingPositionOptions & {
    open: Signal<boolean>;
    anchor: () => FloatingReference | null;
    class?: string;
    role?: any;
    ariaLabel?: string;
    closeOnEscape?: boolean;
    closeOnOutsidePointer?: boolean;
    modalFocus?: boolean;
    initialFocus?: () => HTMLElement | null;
    restoreFocus?: boolean;
    onKeyDown?: (event: preact.TargetedKeyboardEvent<HTMLDivElement>) => void;
    onOpenChange?: (open: boolean, reason: PopoverCloseReason) => void;
    children?: preact.ComponentChildren;
  }

/** Renders unstyled anchored overlay content for feature-owned popovers. */
export component Popover(props: PopoverProps) {
  const popoverId = useId()
  const contentRef = useRef<HTMLDivElement | null>(null)

  function requestClose(reason: PopoverCloseReason) {
    props.open.value = false
    props.onOpenChange?.(false, reason)
  }

  useLayoutEffect(() => {
    if (!props.open.value) {
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
      elements: anchorElement instanceof HTMLElement
        ? [anchorElement, contentElement]
        : [contentElement],
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
    const anchorObserver =
      anchorElement instanceof Element
        ? new MutationObserver(() => {
            if (!document.contains(anchorElement)) {
              requestClose("anchor-lost")
            }
          })
        : null

    anchorObserver?.observe(document, { childList: true, subtree: true })

    const initialFocusElement = props.initialFocus?.()
    if (initialFocusElement) {
      initialFocusElement.focus()
    } else if (!focusFirstElement(contentElement)) {
      contentElement.focus()
    }

    return () => {
      anchorObserver?.disconnect()
      stopPositioning()
      stopDismissal()
      unregisterOverlay()

      if ((props.restoreFocus ?? true) && anchorElement instanceof Element) {
        restoreFocus(anchorElement)
      } else {
        restoreFocus(previouslyFocusedElement)
      }
    }
  }, [props.open.value, popoverId])

  if (!props.open.value) {
    return
  }

  <OverlayPortal root="menu">
    <div
      ref={contentRef}
      aria-label={props.ariaLabel}
      class={props.class}
      role={props.role}
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
