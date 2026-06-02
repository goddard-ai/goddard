export function getShortcutLabel(binding: unknown) {
  if (typeof binding === "string") {
    return binding
  }

  if (binding && typeof binding === "object") {
    if ("combo" in binding && typeof binding.combo === "string") {
      return binding.combo
    }

    if ("sequence" in binding && typeof binding.sequence === "string") {
      return binding.sequence
    }
  }

  return null
}
