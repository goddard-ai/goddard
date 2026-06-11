import type { DaemonSession, SessionLaunchPreviewResponse } from "@goddard-ai/sdk"
import { clamp } from "radashi"

export type SessionConfigOption =
  | NonNullable<SessionLaunchPreviewResponse["configOptions"]>[number]
  | NonNullable<DaemonSession["configOptions"]>[number]
export type SelectSessionConfigOption = Extract<SessionConfigOption, { type: "select" }>

function isConfigOptionGroup(option: unknown): option is {
  name: string
  options: Array<{ value: string; name: string; description?: string | null }>
} {
  return (
    typeof option === "object" &&
    option !== null &&
    "options" in option &&
    Array.isArray(option.options)
  )
}

/** Flattens grouped select options into one ordered list of concrete values. */
export function flattenConfigOptionValues(option: SelectSessionConfigOption) {
  const flattenedOptions: Array<{
    value: string
    name: string
    description?: string | null
  }> = []

  for (const entry of option.options) {
    if (isConfigOptionGroup(entry)) {
      flattenedOptions.push(...entry.options)
      continue
    }

    flattenedOptions.push(entry)
  }

  return flattenedOptions
}

/** Finds one select config option by ACP category. */
export function findSelectConfigOption(
  configOptions: readonly SessionConfigOption[],
  category: string,
) {
  return (
    configOptions.find(
      (option): option is SelectSessionConfigOption =>
        option.category === category && option.type === "select",
    ) ?? null
  )
}

/** Finds the approval/session-mode selector across known ACP adapter encodings. */
export function findSessionModeConfigOption(configOptions: readonly SessionConfigOption[]) {
  return (
    findSelectConfigOption(configOptions, "mode") ??
    configOptions.find(
      (option): option is SelectSessionConfigOption =>
        option.type === "select" &&
        (option.id === "mode" || option.name.trim().toLowerCase() === "approval preset"),
    ) ??
    null
  )
}

export function stepConfigOptionValue(
  option: SelectSessionConfigOption,
  currentValue: string | null,
  direction: -1 | 1,
) {
  const options = flattenConfigOptionValues(option)
  if (options.length === 0) {
    return currentValue
  }

  const currentIndex = options.findIndex((entry) => entry.value === currentValue)
  const nextIndex = clamp(currentIndex < 0 ? 0 : currentIndex + direction, 0, options.length - 1)

  return options[nextIndex]?.value ?? currentValue
}
