const activationFocusSelector = [
  "input[type='search']",
  "[role='searchbox']",
  "[data-workbench-activation-focus='true']",
].join(",")
const searchFocusSelector = ["input[type='search']", "[role='searchbox']"].join(",")

function getFocusableTarget(element: Element) {
  if (!(element instanceof HTMLElement)) {
    return null
  }

  if (element.hidden || element.closest("[hidden], [aria-hidden='true']")) {
    return null
  }

  if (
    element instanceof HTMLInputElement ||
    element instanceof HTMLTextAreaElement ||
    element instanceof HTMLSelectElement ||
    element instanceof HTMLButtonElement
  ) {
    return element.disabled ? null : element
  }

  return element.getAttribute("aria-disabled") === "true" ? null : element
}

function findFocusTarget(container: HTMLElement, selector: string) {
  const targets = container.querySelectorAll(selector)

  for (const target of targets) {
    const focusableTarget = getFocusableTarget(target)

    if (focusableTarget) {
      return focusableTarget
    }
  }

  return null
}

export function findActivationFocusTarget(container: HTMLElement) {
  return findFocusTarget(container, activationFocusSelector)
}

export function findSearchFocusTarget(container: HTMLElement) {
  return findFocusTarget(container, searchFocusSelector)
}
