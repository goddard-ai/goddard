import type { EditablePolicy, KeyEventType } from "powerkeys"

import { AppCommand } from "../commands/app-command.ts"

export const ShortcutBindingCommands = {
  closeActiveTab: AppCommand.closeActiveTab,
  newSession: AppCommand.newSession,
  openKeyboardShortcuts: AppCommand.openKeyboardShortcuts,
  openInbox: AppCommand.openInbox,
  openSessions: AppCommand.openSessions,
  openSearch: AppCommand.openSearch,
  openSpecs: AppCommand.openSpecs,
  openTasks: AppCommand.openTasks,
  openRoadmap: AppCommand.openRoadmap,
} as const

/** Stable shortcut-bindable command ids used across the app keymap and Bun host bridge. */
export type ShortcutBindingCommandId =
  (typeof ShortcutBindingCommands)[keyof typeof ShortcutBindingCommands]

/** One built-in keymap profile identifier. */
export type KeymapProfileId = "goddard"

/** One code-owned shortcut binding definition. */
export type ShortcutBindingDefinition = {
  scope?: string | string[]
  when?: string
  keyEvent?: KeyEventType
  priority?: number
  editablePolicy?: "inherit" | EditablePolicy
  preventDefault?: boolean
  stopPropagation?: boolean
  allowRepeat?: boolean
}

/** One persisted override value for a shortcut-bindable command. */
export type ShortcutKeymapOverride = readonly string[] | null

/** One persisted shortcut keymap file stored under the user-scoped Goddard directory. */
export type UserShortcutKeymapFile = {
  version: 1
  profile: KeymapProfileId
  overrides: Partial<Record<ShortcutBindingCommandId, ShortcutKeymapOverride>>
}

/** One built-in shortcut keymap profile. */
export type ShortcutKeymapProfile = {
  id: KeymapProfileId
  label: string
  bindings: Partial<Record<ShortcutBindingCommandId, readonly string[]>>
}

/** Complete definition table for every declared shortcut-bindable command id. */
export type ShortcutBindingDefinitionTable = {
  [commandId in ShortcutBindingCommandId]: ShortcutBindingDefinition
}

/** Complete built-in profile table keyed by profile id. */
export type ShortcutKeymapProfileTable = {
  [profileId in KeymapProfileId]: ShortcutKeymapProfile
}

/** Effective shortcut expressions after one built-in profile is merged with user overrides. */
export type ResolvedShortcutBindings = Partial<Record<ShortcutBindingCommandId, readonly string[]>>

/** Keyboard binding configuration for each shortcut-bindable command. */
export const shortcutBindingDefinitions: ShortcutBindingDefinitionTable = {
  [ShortcutBindingCommands.closeActiveTab]: {
    when: "workbench.hasClosableActiveTab",
    preventDefault: true,
  },
  [ShortcutBindingCommands.newSession]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openKeyboardShortcuts]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openInbox]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openSessions]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openSearch]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openSpecs]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openTasks]: {
    preventDefault: true,
  },
  [ShortcutBindingCommands.openRoadmap]: {
    preventDefault: true,
  },
}

/** Built-in shortcut keymap profiles shipped by the app. */
export const shortcutKeymapProfiles: ShortcutKeymapProfileTable = {
  goddard: {
    id: "goddard",
    label: "Goddard",
    bindings: {
      [ShortcutBindingCommands.closeActiveTab]: ["Mod+w"],
      [ShortcutBindingCommands.newSession]: ["Mod+n"],
      [ShortcutBindingCommands.openInbox]: ["Alt+1"],
      [ShortcutBindingCommands.openSessions]: ["Alt+2"],
      [ShortcutBindingCommands.openSearch]: ["Alt+3"],
      [ShortcutBindingCommands.openSpecs]: ["Alt+4"],
      [ShortcutBindingCommands.openTasks]: ["Alt+5"],
      [ShortcutBindingCommands.openRoadmap]: ["Alt+6"],
    },
  },
}

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
    Object.values(ShortcutBindingCommands).includes(value as ShortcutBindingCommandId)
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

  for (const commandId of Object.values(ShortcutBindingCommands)) {
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
