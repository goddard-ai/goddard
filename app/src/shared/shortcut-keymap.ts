import type { AppCommandId } from "./app-commands.ts"

/** One built-in keymap profile identifier. */
export type KeymapProfileId = keyof typeof shortcutKeymapProfiles

/** One persisted override value for a shortcut-bindable command. */
export type ShortcutKeymapOverride = readonly string[] | null

/** One persisted shortcut keymap file stored under the user-scoped Goddard directory. */
export type UserShortcutKeymapFile = {
  version: 1
  profile: KeymapProfileId
  overrides: Partial<Record<AppCommandId, ShortcutKeymapOverride>>
}

/** One built-in shortcut keymap profile. */
export type ShortcutKeymapProfile = {
  id: string
  label: string
  bindings: Partial<Record<AppCommandId, readonly string[]>>
}

/** Effective shortcut expressions after one built-in profile is merged with user overrides. */
export type ResolvedShortcutBindings = Partial<Record<AppCommandId, readonly string[]>>

/** Built-in shortcut keymap profiles shipped by the app. */
export const shortcutKeymapProfiles = {
  goddard: {
    id: "goddard",
    label: "Goddard",
    bindings: {
      closeActiveTab: ["Mod+w"],
      newSession: ["Mod+n"],
      openInbox: ["Alt+1"],
      openSessions: ["Alt+2"],
      openSearch: ["Alt+3"],
      openSpecs: ["Alt+4"],
      openTasks: ["Alt+5"],
      openRoadmap: ["Alt+6"],
    },
  },
} satisfies Record<string, ShortcutKeymapProfile>

/** Returns one empty user keymap file using the default built-in profile. */
export function createDefaultShortcutKeymapFile(): UserShortcutKeymapFile {
  return {
    version: 1,
    profile: "goddard",
    overrides: {},
  }
}

/** Returns whether one runtime string matches a known shortcut-bindable command id. */
export function isShortcutBindingCommandId(value: unknown): value is ShortcutBindingCommandId {
  return (
    typeof value === "string" &&
    shortcutBindingCommandIds.includes(value as ShortcutBindingCommandId)
  )
}

/** Returns whether one runtime string matches a shipped keymap profile id. */
export function isKeymapProfileId(value: unknown): value is KeymapProfileId {
  return typeof value === "string" && value in shortcutKeymapProfiles
}

/** Parses one unknown JSON payload into a shortcut keymap file when valid. */
export function parseShortcutKeymapFile(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null
  }

  const candidate = value as {
    version?: unknown
    profile?: unknown
    overrides?: unknown
  }

  if (candidate.version !== 1 || !isKeymapProfileId(candidate.profile)) {
    return null
  }

  if (
    !candidate.overrides ||
    typeof candidate.overrides !== "object" ||
    Array.isArray(candidate.overrides)
  ) {
    return null
  }

  const overrides: Partial<Record<ShortcutBindingCommandId, ShortcutKeymapOverride>> = {}

  for (const [commandId, expressionList] of Object.entries(candidate.overrides)) {
    if (!isShortcutBindingCommandId(commandId)) {
      return null
    }

    if (expressionList === null) {
      overrides[commandId] = null
      continue
    }

    if (
      !Array.isArray(expressionList) ||
      expressionList.length === 0 ||
      expressionList.some((expression) => typeof expression !== "string" || expression.length === 0)
    ) {
      return null
    }

    overrides[commandId] = expressionList
  }

  return {
    version: 1,
    profile: candidate.profile,
    overrides,
  }
}

/** Resolves one built-in profile plus optional user overrides into effective bindings. */
export function resolveShortcutBindings(
  profileId: KeymapProfileId,
  overrides: Partial<Record<ShortcutBindingCommandId, ShortcutKeymapOverride>> = {},
) {
  const resolvedBindings = { ...shortcutKeymapProfiles[profileId].bindings }

  for (const commandId of shortcutBindingCommandIds) {
    const override = overrides[commandId]

    if (override === undefined) {
      continue
    }

    if (override === null) {
      delete resolvedBindings[commandId]
      continue
    }

    resolvedBindings[commandId] = override
  }

  return resolvedBindings
}
