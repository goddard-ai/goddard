import { expect, test } from "bun:test"

import {
  createDefaultShortcutKeymapFile,
  parseShortcutKeymapFile,
  resolveShortcutBindings,
} from "./shortcut-keymap.ts"

const newSession = "navigation.openNewSessionDialog" as const
const openInbox = "navigation.openInbox" as const
const openKeyboardShortcuts = "navigation.openKeyboardShortcuts" as const
const closeActiveTab = "workbench.closeActiveTab" as const
const openSessions = "navigation.openSessions" as const
const openSearch = "navigation.openSearch" as const
const openSpecs = "navigation.openSpecs" as const
const openTasks = "navigation.openTasks" as const
const openRoadmap = "navigation.openRoadmap" as const

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

test("parseShortcutKeymapFile ignores unknown command ids and rejects empty override arrays", () => {
  expect(
    parseShortcutKeymapFile({
      version: 1,
      profile: "goddard",
      overrides: {
        unknown: ["Mod+k"],
      },
    }),
  ).toEqual({
    version: 1,
    profile: "goddard",
    overrides: {},
  })

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
