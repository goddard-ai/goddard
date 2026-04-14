import { expect, test } from "bun:test"

import {
  createDefaultShortcutKeymapFile,
  parseShortcutKeymapFile,
  resolveShortcutBindings,
  ShortcutBindingCommands,
} from "./shortcut-keymap.ts"

test("parseShortcutKeymapFile accepts a valid persisted keymap", () => {
  expect(
    parseShortcutKeymapFile({
      version: 1,
      profile: "goddard",
      overrides: {
        [ShortcutBindingCommands.newSession]: ["Mod+Shift+n"],
        [ShortcutBindingCommands.openKeyboardShortcuts]: null,
      },
    }),
  ).toEqual({
    version: 1,
    profile: "goddard",
    overrides: {
      [ShortcutBindingCommands.newSession]: ["Mod+Shift+n"],
      [ShortcutBindingCommands.openKeyboardShortcuts]: null,
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
        [ShortcutBindingCommands.newSession]: [],
      },
    }),
  ).toBeNull()
})

test("resolveShortcutBindings applies unbind and replacement overrides over the built-in profile", () => {
  const defaultFile = createDefaultShortcutKeymapFile()

  expect(
    resolveShortcutBindings(defaultFile.profile, {
      [ShortcutBindingCommands.newSession]: ["Mod+Shift+n"],
      [ShortcutBindingCommands.openInbox]: null,
    }),
  ).toEqual({
    [ShortcutBindingCommands.closeActiveTab]: ["Mod+w"],
    [ShortcutBindingCommands.newSession]: ["Mod+Shift+n"],
    [ShortcutBindingCommands.openSessions]: ["Alt+2"],
    [ShortcutBindingCommands.openSearch]: ["Alt+3"],
    [ShortcutBindingCommands.openSpecs]: ["Alt+4"],
    [ShortcutBindingCommands.openTasks]: ["Alt+5"],
    [ShortcutBindingCommands.openRoadmap]: ["Alt+6"],
  })
})
