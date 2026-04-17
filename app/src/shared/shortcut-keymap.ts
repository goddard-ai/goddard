import type { BindingInput } from "powerkeys"
import { z } from "zod"

import type { AppCommandId } from "./app-commands.ts"

export type ShortcutBindingObject = (
  | { combo: string; sequence?: undefined }
  | { sequence: string; combo?: undefined }
) &
  Omit<Extract<BindingInput, object>, "handler">

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
      "navigation.openInbox": ["Alt+1"],
      "navigation.openSessions": ["Alt+2"],
      "navigation.openSearch": ["Alt+3"],
      "navigation.openSpecs": ["Alt+4"],
      "navigation.openTasks": ["Alt+5"],
      "navigation.openRoadmap": ["Alt+6"],
      "navigation.openCommandPalette": ["Mod+p"],
      "navigation.openNewSessionDialog": ["Mod+n"],
      "navigation.openSwitchProject": ["Mod+o"],
      "sessionInput.openProjectSelector": ["Mod+p"],
      "sessionInput.openAdapterSelector": ["Mod+Shift+a"],
      "sessionInput.openLocationSelector": ["Mod+Shift+l"],
      "sessionInput.openBranchSelector": ["Mod+Shift+b"],
      "sessionInput.openModelSelector": ["Mod+Shift+m"],
      "sessionInput.openThinkingLevelSelector": ["Mod+t"],
      "sessionInput.submit": ["Mod+Enter"],
      "workbench.closeActiveTab": ["Mod+w"],
    },
  },
} satisfies Record<string, ShortcutKeymapProfile>

const defaultKeymapProfileId = "goddard" satisfies KeymapProfileId

const ShortcutBindingObject = z.union([
  z.looseObject({
    combo: z.string().min(1),
    sequence: z.undefined().optional(),
  }),
  z.looseObject({
    sequence: z.string().min(1),
    combo: z.undefined().optional(),
  }),
]) as z.ZodType<ShortcutBindingObject>

const ShortcutBinding = z.union([z.string().min(1), ShortcutBindingObject])

export type ShortcutBinding = z.infer<typeof ShortcutBinding>

export type ShortcutKeymapBindings = Partial<Record<AppCommandId, readonly ShortcutBinding[]>>

export const ShortcutKeymapOverrides = z.record(
  z.string(),
  z.union([z.array(ShortcutBinding).nonempty(), z.null()]),
)

export type ShortcutKeymapOverrides = z.infer<typeof ShortcutKeymapOverrides>

/** Returns one empty user keymap file using the default built-in profile. */
export function createDefaultShortcutKeymapFile(): UserShortcutKeymapFile {
  return {
    version: 1,
    profile: defaultKeymapProfileId,
    overrides: {},
  }
}

/** Returns whether one runtime string matches a shipped keymap profile id. */
export function isKeymapProfileId(value: unknown): value is KeymapProfileId {
  return typeof value === "string" && value in shortcutKeymapProfiles
}

/** Schema for one persisted shortcut keymap file stored under the user-scoped Goddard directory. */
export const UserShortcutKeymapFile = z.object({
  version: z.literal(1),
  profile: z.preprocess(
    (value: unknown) => (isKeymapProfileId(value) ? value : defaultKeymapProfileId),
    z.custom<KeymapProfileId>(isKeymapProfileId),
  ),
  overrides: ShortcutKeymapOverrides,
})

export type UserShortcutKeymapFile = z.infer<typeof UserShortcutKeymapFile>

/** Resolves one built-in profile plus optional user overrides into effective bindings. */
export function resolveShortcutBindings(
  profileId: KeymapProfileId,
  overrides: ShortcutKeymapOverrides = {},
) {
  const profileBindings = shortcutKeymapProfiles[profileId].bindings
  const resolvedBindings: ShortcutKeymapBindings = {
    ...profileBindings,
  }

  for (const commandId of Object.keys(overrides)) {
    if (!(commandId in profileBindings)) {
      continue
    }

    const override = overrides[commandId]
    const typedCommandId = commandId as AppCommandId

    if (override === undefined) {
      continue
    }
    if (override === null) {
      delete resolvedBindings[typedCommandId]
    } else {
      resolvedBindings[typedCommandId] = override
    }
  }

  return resolvedBindings
}
