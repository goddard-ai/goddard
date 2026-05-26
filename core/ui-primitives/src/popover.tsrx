import { type Signal } from "@preact/signals"
import { useId, useLayoutEffect, useRef } from "preact/hooks"

import { startOverlayDismissal } from "./overlay/dismissal.ts"
import { focusFirstElement, keepFocusInside, restoreFocus } from "./overlay/focus.ts"
import { OverlayPortal } from "./overlay/portal.ts"
import {
  startFloatingPosition,
  type FloatingPositionOptions,
  type FloatingReference,
} from "./overlay/position.ts"
import { overlayStack } from "./overlay/stack.ts"

export type PopoverCloseReason = "escape" | "outside"

export type PopoverProps = FloatingPositionOptions & {
    open: Signal<boolean>;
    anchor: () => FloatingReference | null;
    class?: string;
    role?: any;
    ariaLabel?: string;
    blockOutsidePointer?: boolean;
    closeOnEscape?: boolean;
    closeOnOutsidePointer?: boolean;
    focusOnOpen?: boolean;
    modalFocus?: boolean;
    initialFocus?: () => HTMLElement | null;
    restoreFocus?: boolean;
    onKeyDown?: (event: preact.TargetedKeyboardEvent<HTMLDivElement>) => void;
    onOpenChange?: (open: boolean, reason: PopoverCloseReason) => void;
    children?: preact.ComponentChildren;
  }

/** Renders unstyled anchored overlay content for feature-owned popovers. */
export component Popover(props: PopoverProps) {
  if (!props.open.value) {
    return
  }

  const popoverId = useId()
  const contentRef = useRef<HTMLDivElement | null>(null)
  const close = (reason: PopoverCloseReason) => {
    props.open.value = false
    props.onOpenChange?.(false, reason)
  }

  useLayoutEffect(() => {
    const anchorElement = props.anchor()
    const contentElement = contentRef.current

    if (!anchorElement || !contentElement) {
      console.error("Popover opened without a mounted anchor or content element.", {
        hasAnchor: Boolean(anchorElement),
        hasContent: Boolean(contentElement),
      })
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
      close,
    })
    const stopPositioning = startFloatingPosition(anchorElement, contentElement, {
      offset: props.offset,
      placement: props.placement,
      sameWidth: props.sameWidth,
      strategy: props.strategy,
    })
    if (props.focusOnOpen ?? true) {
      const initialFocusElement = props.initialFocus?.()
      if (initialFocusElement) {
        initialFocusElement.focus()
      } else if (!focusFirstElement(contentElement)) {
        contentElement.focus()
      }
    }

    return () => {
      stopPositioning()
      stopDismissal()
      unregisterOverlay()

      if ((props.restoreFocus ?? true) && anchorElement instanceof Element) {
        restoreFocus(anchorElement)
      } else {
        restoreFocus(previouslyFocusedElement)
      }
    }
  }, [popoverId])

  <OverlayPortal root="menu">
    const shouldBlockOutsidePointer =
      props.blockOutsidePointer ?? props.closeOnOutsidePointer ?? true

    if (shouldBlockOutsidePointer) {
      <div
        aria-hidden="true"
        data-overlay-pointer-blocker="true"
        style={{
          position: "fixed",
          inset: "0",
          background: "transparent",
          pointerEvents: "auto",
        }}
      />
    }
    <div
      ref={contentRef}
      aria-label={props.ariaLabel}
      class={props.class}
      role={props.role}
      tabIndex={props.focusOnOpen ?? true ? -1 : undefined}
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
