export const ShortcutCommands = {
  closeActiveTab: "workbench.closeActiveTab",
  newSession: "session.new",
  openKeyboardShortcuts: "workbench.openKeyboardShortcuts",
  openInbox: "navigation.openInbox",
  openSessions: "navigation.openSessions",
  openSearch: "navigation.openSearch",
  openSpecs: "navigation.openSpecs",
  openTasks: "navigation.openTasks",
  openRoadmap: "navigation.openRoadmap",
} as const

/** Stable shortcut command ids used across the app, keymap storage, and Bun host bridge. */
export type ShortcutCommandId = (typeof ShortcutCommands)[keyof typeof ShortcutCommands]

/** One built-in keymap profile identifier. */
export type KeymapProfileId = "goddard"

/** One code-owned shortcut command definition. */
export type ShortcutCommandDefinition = {
  label: string
  description: string
  nativeMenuAction?: boolean
}

/** One persisted override value for a shortcut command. */
export type ShortcutKeymapOverride = readonly string[] | null

/** One persisted shortcut keymap file stored under the user-scoped Goddard directory. */
export type UserShortcutKeymapFile = {
  version: 1
  profile: KeymapProfileId
  overrides: Partial<Record<ShortcutCommandId, ShortcutKeymapOverride>>
}

/** One built-in shortcut keymap profile. */
export type ShortcutKeymapProfile = {
  id: KeymapProfileId
  label: string
  bindings: Partial<Record<ShortcutCommandId, readonly string[]>>
}

/** Human-facing metadata for each shortcut command. */
export const shortcutCommandDefinitions: Record<ShortcutCommandId, ShortcutCommandDefinition> = {
  [ShortcutCommands.closeActiveTab]: {
    label: "Close Active Tab",
    description: "Closes the current closable workbench tab.",
    nativeMenuAction: true,
  },
  [ShortcutCommands.newSession]: {
    label: "New Session",
    description: "Opens the new-session dialog from the app shell.",
  },
  [ShortcutCommands.openKeyboardShortcuts]: {
    label: "Open Keyboard Shortcuts",
    description: "Opens the keyboard shortcut browser in a detail tab.",
  },
  [ShortcutCommands.openInbox]: {
    label: "Open Inbox",
    description: "Selects the Inbox main workbench view.",
  },
  [ShortcutCommands.openSessions]: {
    label: "Open Sessions",
    description: "Selects the Sessions main workbench view.",
  },
  [ShortcutCommands.openSearch]: {
    label: "Open Search",
    description: "Selects the Search main workbench view.",
  },
  [ShortcutCommands.openSpecs]: {
    label: "Open Specs",
    description: "Selects the Specs main workbench view.",
  },
  [ShortcutCommands.openTasks]: {
    label: "Open Tasks",
    description: "Selects the Tasks main workbench view.",
  },
  [ShortcutCommands.openRoadmap]: {
    label: "Open Roadmap",
    description: "Selects the Roadmap main workbench view.",
  },
}

/** Built-in shortcut keymap profiles shipped by the app. */
export const shortcutKeymapProfiles: Record<KeymapProfileId, ShortcutKeymapProfile> = {
  goddard: {
    id: "goddard",
    label: "Goddard",
    bindings: {
      [ShortcutCommands.closeActiveTab]: ["Mod+w"],
      [ShortcutCommands.newSession]: ["Mod+n"],
      [ShortcutCommands.openInbox]: ["Alt+1"],
      [ShortcutCommands.openSessions]: ["Alt+2"],
      [ShortcutCommands.openSearch]: ["Alt+3"],
      [ShortcutCommands.openSpecs]: ["Alt+4"],
      [ShortcutCommands.openTasks]: ["Alt+5"],
      [ShortcutCommands.openRoadmap]: ["Alt+6"],
    },
  },
}

/** Returns one empty user keymap file using the default built-in profile. */
export function createDefaultShortcutKeymapFile() {
  return {
    version: 1,
    profile: "goddard",
    overrides: {},
  }
}

/** Returns whether one runtime string matches a known shortcut command id. */
export function isShortcutCommandId(value: unknown): value is ShortcutCommandId {
  return (
    typeof value === "string" &&
    Object.values(ShortcutCommands).includes(value as ShortcutCommandId)
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

  const overrides: Partial<Record<ShortcutCommandId, ShortcutKeymapOverride>> = {}

  for (const [commandId, expressionList] of Object.entries(candidate.overrides)) {
    if (!isShortcutCommandId(commandId)) {
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
  overrides: Partial<Record<ShortcutCommandId, ShortcutKeymapOverride>> = {},
) {
  const resolvedBindings = { ...shortcutKeymapProfiles[profileId].bindings }

  for (const commandId of Object.values(ShortcutCommands)) {
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
