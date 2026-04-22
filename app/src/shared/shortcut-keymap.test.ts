import { expect, test } from "bun:test"

import {
  createDefaultShortcutKeymapFile,
  resolveShortcutBindings,
  UserShortcutKeymapFile,
} from "./shortcut-keymap.ts"

const newSession = "navigation.openNewSessionDialog" as const
const openSwitchProject = "navigation.openSwitchProject" as const
const openInbox = "navigation.openInbox" as const
const openKeyboardShortcuts = "navigation.openKeyboardShortcuts" as const
const closeActiveTab = "workbench.closeActiveTab" as const
const openSessions = "navigation.openSessions" as const
const openSearch = "navigation.openSearch" as const
const openSpecs = "navigation.openSpecs" as const
const openTasks = "navigation.openTasks" as const
const openRoadmap = "navigation.openRoadmap" as const
const openCommandPalette = "navigation.openCommandPalette" as const
const openProjectSelector = "sessionInput.openProjectSelector" as const
const openAdapterSelector = "sessionInput.openAdapterSelector" as const
const openLocationSelector = "sessionInput.openLocationSelector" as const
const openBranchSelector = "sessionInput.openBranchSelector" as const
const openModelSelector = "sessionInput.openModelSelector" as const
const openThinkingLevelSelector = "sessionInput.openThinkingLevelSelector" as const
const submitSessionInput = "sessionInput.submit" as const

test("parseShortcutKeymapFile accepts a valid persisted keymap", () => {
  expect(
    UserShortcutKeymapFile.safeParse({
      version: 1,
      profile: "goddard",
      overrides: {
        [newSession]: ["Mod+Shift+n"],
        [openKeyboardShortcuts]: null,
      },
    }),
  ).toEqual({
    success: true,
    data: {
      version: 1,
      profile: "goddard",
      overrides: {
        [newSession]: ["Mod+Shift+n"],
        [openKeyboardShortcuts]: null,
      },
    },
  })
})

test("parseShortcutKeymapFile keeps unknown command ids and rejects empty override arrays", () => {
  expect(
    UserShortcutKeymapFile.safeParse({
      version: 1,
      profile: "goddard",
      overrides: {
        unknown: ["Mod+k"],
      },
    }),
  ).toEqual({
    success: true,
    data: {
      version: 1,
      profile: "goddard",
      overrides: {
        unknown: ["Mod+k"],
      },
    },
  })

  expect(
    UserShortcutKeymapFile.safeParse({
      version: 1,
      profile: "goddard",
      overrides: {
        [newSession]: [],
      },
    }).success,
  ).toBe(false)
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
    [openCommandPalette]: ["Mod+p"],
    [newSession]: ["Mod+Shift+n"],
    [openSwitchProject]: ["Mod+o"],
    [openSessions]: ["Alt+2"],
    [openSearch]: ["Alt+3"],
    [openSpecs]: ["Alt+4"],
    [openTasks]: ["Alt+5"],
    [openRoadmap]: ["Alt+6"],
    [openProjectSelector]: ["Mod+p"],
    [openAdapterSelector]: ["Mod+Shift+a"],
    [openLocationSelector]: ["Mod+Shift+l"],
    [openBranchSelector]: ["Mod+Shift+b"],
    [openModelSelector]: ["Mod+Shift+m"],
    [openThinkingLevelSelector]: ["Mod+t"],
    [submitSessionInput]: ["Mod+Enter"],
  })
})

test("resolveShortcutBindings allows overrides for known commands that ship without defaults", () => {
  const defaultFile = createDefaultShortcutKeymapFile()

  expect(
    resolveShortcutBindings(defaultFile.profile, {
      [openKeyboardShortcuts]: ["Mod+/"],
      unknown: ["Alt+/"],
    }),
  ).toMatchObject({
    [closeActiveTab]: ["Mod+w"],
    [openInbox]: ["Alt+1"],
    [openKeyboardShortcuts]: ["Mod+/"],
    [openThinkingLevelSelector]: ["Mod+t"],
    unknown: ["Alt+/"],
  })
})
