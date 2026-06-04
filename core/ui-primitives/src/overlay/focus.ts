import { tabbable } from "tabbable"

const restoredFocusTargets = new WeakSet<HTMLElement>()

/** Moves focus to the first reachable control inside a newly opened overlay. */
export function focusFirstElement(container: HTMLElement) {
  const firstElement = tabbable(container)[0]

  if (!firstElement) {
    return false
  }

  firstElement.focus()
  return true
}

/** Returns focus to a trigger when the trigger is still mounted. */
export function restoreFocus(target: Element | null) {
  if (target instanceof HTMLElement && document.contains(target)) {
    restoredFocusTargets.add(target)
    target.focus()
  }
}

/** Returns true once for an element focused by overlay restoration. */
export function consumeRestoredFocus(target: EventTarget | null) {
  if (!(target instanceof HTMLElement) || !restoredFocusTargets.has(target)) {
    return false
  }

  restoredFocusTargets.delete(target)
  return true
}

/** Loops tab navigation inside an overlay that owns modal focus. */
export function keepFocusInside(container: HTMLElement, event: KeyboardEvent) {
  if (event.key !== "Tab") {
    return
  }

  const focusableElements = tabbable(container)
  const firstElement = focusableElements[0]
  const lastElement = focusableElements.at(-1)

  if (!firstElement || !lastElement) {
    event.preventDefault()
    container.focus()
    return
  }

  if (event.shiftKey && document.activeElement === firstElement) {
    event.preventDefault()
    lastElement.focus()
    return
  }

  if (!event.shiftKey && document.activeElement === lastElement) {
    event.preventDefault()
    firstElement.focus()
  }
}
