import { expect, test } from "bun:test"

import {
  GLOBAL_SESSION_LAUNCH_SHORTCUT_DEFAULT_BINDING,
  GlobalSessionLaunchShortcut,
} from "./global-shortcut.ts"

test("global session launch shortcut starts disabled with the suggested binding", () => {
  const shortcut = new GlobalSessionLaunchShortcut()

  expect(shortcut.enabled).toBe(false)
  expect(shortcut.binding).toBe(GLOBAL_SESSION_LAUNCH_SHORTCUT_DEFAULT_BINDING)
  expect(shortcut.registration).toEqual({
    status: "unregistered",
    error: null,
  })
})

test("global session launch shortcut cannot enable until a project exists", () => {
  const shortcut = new GlobalSessionLaunchShortcut()

  expect(shortcut.enable(0)).toBe(false)
  expect(shortcut.enabled).toBe(false)

  expect(shortcut.enable(1)).toBe(true)
  expect(shortcut.enabled).toBe(true)
})

test("global session launch shortcut records conflicts as disabled inline errors", () => {
  const shortcut = new GlobalSessionLaunchShortcut()

  shortcut.enable(1)
  shortcut.markRegistrationUnavailable("Command+Period is already in use.")

  expect(shortcut.enabled).toBe(false)
  expect(shortcut.registrationError).toBe("Command+Period is already in use.")
  expect(shortcut.registration).toEqual({
    status: "unavailable",
    error: "Command+Period is already in use.",
  })
})

test("global session launch shortcut binding edits reset registration status", () => {
  const shortcut = new GlobalSessionLaunchShortcut()

  shortcut.enable(1)
  shortcut.markRegistered()
  shortcut.setBinding(" Command+Space ")

  expect(shortcut.enabled).toBe(true)
  expect(shortcut.binding).toBe("Command+Space")
  expect(shortcut.registration).toEqual({
    status: "unregistered",
    error: null,
  })
})
