import type { DaemonSession, SessionLaunchPreviewResponse } from "@goddard-ai/sdk"

type SessionConfigOption =
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
