import { expect, test } from "vitest"
import {
  createDefaultShortcutKeymapFile,
  parseShortcutKeymapFile,
  resolveShortcutBindings,
  ShortcutCommands,
} from "./shortcut-keymap.ts"

test("parseShortcutKeymapFile accepts a valid persisted keymap", () => {
  expect(
    parseShortcutKeymapFile({
      version: 1,
      profile: "goddard",
      overrides: {
        [ShortcutCommands.newSession]: ["Mod+Shift+n"],
        [ShortcutCommands.openKeyboardShortcuts]: null,
      },
    }),
  ).toEqual({
    version: 1,
    profile: "goddard",
    overrides: {
      [ShortcutCommands.newSession]: ["Mod+Shift+n"],
      [ShortcutCommands.openKeyboardShortcuts]: null,
    },
  })
})

test("parseShortcutKeymapFile rejects empty override arrays and unknown command ids", () => {
  expect(
    parseShortcutKeymapFile({
      version: 1,
      profile: "goddard",
      overrides: {
        unknown: ["Mod+k"],
      },
    }),
  ).toBeNull()

  expect(
    parseShortcutKeymapFile({
      version: 1,
      profile: "goddard",
      overrides: {
        [ShortcutCommands.newSession]: [],
      },
    }),
  ).toBeNull()
})

test("resolveShortcutBindings applies unbind and replacement overrides over the built-in profile", () => {
  const defaultFile = createDefaultShortcutKeymapFile()

  expect(
    resolveShortcutBindings(defaultFile.profile, {
      [ShortcutCommands.newSession]: ["Mod+Shift+n"],
      [ShortcutCommands.openInbox]: null,
    }),
  ).toEqual({
    [ShortcutCommands.closeActiveTab]: ["Mod+w"],
    [ShortcutCommands.newSession]: ["Mod+Shift+n"],
    [ShortcutCommands.openSessions]: ["Alt+2"],
    [ShortcutCommands.openSearch]: ["Alt+3"],
    [ShortcutCommands.openSpecs]: ["Alt+4"],
    [ShortcutCommands.openTasks]: ["Alt+5"],
    [ShortcutCommands.openRoadmap]: ["Alt+6"],
  })
})
