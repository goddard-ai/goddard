import type { BindingInput } from "powerkeys"
import { z } from "zod"

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

export type ShortcutKeymapBindings = Partial<Record<string, readonly ShortcutBinding[]>>

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
  const resolvedBindings: ShortcutKeymapBindings = {
    ...shortcutKeymapProfiles[profileId].bindings,
  }

  for (const commandId of Object.keys(overrides)) {
    const override = overrides[commandId]
    if (override === undefined) {
      continue
    }
    if (override === null) {
      delete resolvedBindings[commandId]
    } else {
      resolvedBindings[commandId] = override
    }
  }

  return resolvedBindings
}
