import { type Signal } from "@preact/signals"
import { useId, useLayoutEffect, useRef } from "preact/hooks"

import { startOverlayDismissal } from "./overlay/dismissal.ts"
import { focusFirstElement, keepFocusInside, restoreFocus } from "./overlay/focus.ts"
import { OverlayPortal } from "./overlay/portal.tsrx"
import { overlayStack } from "./overlay/stack.ts"

export type ModalCloseReason = "escape" | "explicit" | "outside"

/** Configuration for one unstyled modal surface. */
export type ModalProps = {
    open: Signal<boolean>;
    titleId: string;
    descriptionId?: string;
    backdropClass?: string;
    positionerClass?: string;
    contentClass?: string;
    closeOnEscape?: boolean;
    closeOnOutsidePointer?: boolean;
    initialFocus?: () => HTMLElement | null;
    onBeforeClose?: (reason: ModalCloseReason) => boolean;
    children?: preact.ComponentChildren;
  }

/** Renders one modal dialog with owned focus, dismissal, and portal behavior. */
export component Modal(props: ModalProps) {
  const modalId = useId()
  const contentRef = useRef<HTMLDivElement | null>(null)

  function requestClose(reason: ModalCloseReason) {
    if (props.onBeforeClose?.(reason) === false) {
      return
    }

    props.open.value = false
  }

  useLayoutEffect(() => {
    if (!props.open.value) {
      return
    }

    const contentElement = contentRef.current

    if (!contentElement) {
      return
    }

    const previouslyFocusedElement = document.activeElement
    const unregisterOverlay = overlayStack.register({
      id: modalId,
      elements: [contentElement],
    })
    const stopDismissal = startOverlayDismissal({
      id: modalId,
      dismissOnEscape: props.closeOnEscape ?? true,
      dismissOnOutsidePointer: props.closeOnOutsidePointer ?? false,
      close(reason) {
        requestClose(reason)
      },
    })

    const initialFocusElement = props.initialFocus?.()
    if (initialFocusElement) {
      initialFocusElement.focus()
    } else if (!focusFirstElement(contentElement)) {
      contentElement.focus()
    }

    return () => {
      stopDismissal()
      unregisterOverlay()
      restoreFocus(previouslyFocusedElement)
    }
  }, [modalId, props.open.value])

  if (!props.open.value) {
    return
  }

  <OverlayPortal root="dialog">
    <div aria-hidden="true" class={props.backdropClass} />
    <div class={props.positionerClass}>
      <div
        ref={contentRef}
        aria-describedby={props.descriptionId}
        aria-labelledby={props.titleId}
        aria-modal="true"
        class={props.contentClass}
        role="dialog"
        tabIndex={-1}
        onKeyDown={(event) => {
          keepFocusInside(event.currentTarget, event)
        }}
      >
        {props.children}
      </div>
    </div>
  </OverlayPortal>
}
