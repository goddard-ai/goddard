import { BindingInput } from "powerkeys"

import { appCommandList, resolveAppCommand } from "~/commands/app-command.ts"
import type { AppCommandId } from "./app-commands.ts"

export type ShortcutBindingObject = (
  | { combo: string; sequence?: undefined }
  | { sequence: string; combo?: undefined }
) &
  Omit<Extract<BindingInput, object>, "handler">

export type ShortcutBinding = string | ShortcutBindingObject

export type ShortcutKeymapBindings = Partial<Record<AppCommandId, readonly ShortcutBinding[]>>

/** One built-in keymap profile identifier. */
export type KeymapProfileId = keyof typeof shortcutKeymapProfiles

/** One built-in shortcut keymap profile. */
export type ShortcutKeymapProfile = {
  id: string
  label: string
  bindings: ShortcutKeymapBindings
}

/** Built-in shortcut keymap profiles shipped by the app. */
export const shortcutKeymapProfiles = {
  goddard: {
    id: "goddard",
    label: "Goddard",
    bindings: {
      "workbench.closeActiveTab": ["Mod+w"],
      "navigation.openNewSessionDialog": ["Mod+n"],
      "navigation.openInbox": ["Alt+1"],
      "navigation.openSessions": ["Alt+2"],
      "navigation.openSearch": ["Alt+3"],
      "navigation.openSpecs": ["Alt+4"],
      "navigation.openTasks": ["Alt+5"],
      "navigation.openRoadmap": ["Alt+6"],
    },
  },
} satisfies Record<string, ShortcutKeymapProfile>

export type ShortcutKeymapOverrides = Partial<
  Record<AppCommandId, readonly ShortcutBinding[] | null>
>

/** One persisted shortcut keymap file stored under the user-scoped Goddard directory. */
export type UserShortcutKeymapFile = {
  version: 1
  profile: KeymapProfileId
  overrides: ShortcutKeymapOverrides
}

/** Returns one empty user keymap file using the default built-in profile. */
export function createDefaultShortcutKeymapFile(): UserShortcutKeymapFile {
  return {
    version: 1,
    profile: "goddard",
    overrides: {},
  }
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

  if (candidate.version !== 1) {
    return null
  }

  if (
    !candidate.overrides ||
    typeof candidate.overrides !== "object" ||
    Array.isArray(candidate.overrides)
  ) {
    return null
  }

  if (!isKeymapProfileId(candidate.profile)) {
    candidate.profile = "goddard" satisfies KeymapProfileId
  }

  const overrides: ShortcutKeymapOverrides = {}

  for (const [commandId, expressionList] of Object.entries(candidate.overrides)) {
    const command = resolveAppCommand(commandId as AppCommandId)
    if (!command) {
      continue
    }

    if (expressionList === null) {
      overrides[command.id] = null
      continue
    }

    if (
      !Array.isArray(expressionList) ||
      expressionList.length === 0 ||
      expressionList.some((expression) => typeof expression !== "string" || expression.length === 0)
    ) {
      return null
    }

    overrides[command.id] = expressionList
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
  overrides: ShortcutKeymapOverrides = {},
) {
  const resolvedBindings: ShortcutKeymapBindings = {
    ...shortcutKeymapProfiles[profileId].bindings,
  }

  for (const command of appCommandList) {
    const override = overrides[command.id]
    if (override === undefined) {
      continue
    }
    if (override === null) {
      delete resolvedBindings[command.id]
    } else {
      resolvedBindings[command.id] = override
    }
  }

  return resolvedBindings
}
