import { expect, test } from "bun:test"

import {
  createDefaultShortcutKeymapFile,
  parseShortcutKeymapFile,
  resolveShortcutBindings,
} from "./shortcut-keymap.ts"

const newSession = "newSession" as const
const openInbox = "openInbox" as const
const openKeyboardShortcuts = "openKeyboardShortcuts" as const
const closeActiveTab = "closeActiveTab" as const
const openSessions = "openSessions" as const
const openSearch = "openSearch" as const
const openSpecs = "openSpecs" as const
const openTasks = "openTasks" as const
const openRoadmap = "openRoadmap" as const

test("parseShortcutKeymapFile accepts a valid persisted keymap", () => {
  expect(
    parseShortcutKeymapFile({
      version: 1,
      profile: "goddard",
      overrides: {
        [newSession]: ["Mod+Shift+n"],
        [openKeyboardShortcuts]: null,
      },
    }),
  ).toEqual({
    version: 1,
    profile: "goddard",
    overrides: {
      [newSession]: ["Mod+Shift+n"],
      [openKeyboardShortcuts]: null,
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
        [newSession]: [],
      },
    }),
  ).toBeNull()
})

test("resolveShortcutBindings applies unbind and replacement overrides over the built-in profile", () => {
  const defaultFile = createDefaultShortcutKeymapFile()

  expect(
    resolveShortcutBindings(defaultFile.profile, {
      [newSession]: ["Mod+Shift+n"],
      [openInbox]: null,
    }),
  ).toEqual({
    [closeActiveTab]: ["Mod+w"],
    [newSession]: ["Mod+Shift+n"],
    [openSessions]: ["Alt+2"],
    [openSearch]: ["Alt+3"],
    [openSpecs]: ["Alt+4"],
    [openTasks]: ["Alt+5"],
    [openRoadmap]: ["Alt+6"],
  })
})
