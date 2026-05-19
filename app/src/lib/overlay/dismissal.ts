import { overlayStack } from "./stack.ts"

/** Dismissal behavior owned by one overlay while it is mounted. */
export type OverlayDismissalOptions = {
  id: string
  close: (reason: "escape" | "outside") => void
  dismissOnEscape?: boolean
  dismissOnOutsidePointer?: boolean
}

/** Registers document-level dismissal listeners for the topmost overlay only. */
export function startOverlayDismissal(options: OverlayDismissalOptions) {
  function closeIfTopmost(reason: "escape" | "outside") {
    if (overlayStack.isTopmost(options.id)) {
      options.close(reason)
    }
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (options.dismissOnEscape === false || event.key !== "Escape") {
      return
    }

    event.preventDefault()
    closeIfTopmost("escape")
  }

  function handlePointerDown(event: PointerEvent) {
    if (options.dismissOnOutsidePointer === false || overlayStack.contains(event.target)) {
      return
    }

    closeIfTopmost("outside")
  }

  document.addEventListener("keydown", handleKeyDown)
  document.addEventListener("pointerdown", handlePointerDown, true)

  return () => {
    document.removeEventListener("keydown", handleKeyDown)
    document.removeEventListener("pointerdown", handlePointerDown, true)
  }
}
