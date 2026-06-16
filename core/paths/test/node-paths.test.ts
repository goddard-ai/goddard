import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import {
  getAppStatePath,
  getDatabasePath,
  getGoddardLogDatabasePath,
  getGoddardLogDir,
  getGoddardTempLogDir,
} from "../src/node/index.ts"

const originalHome = process.env.HOME
const originalNodeEnv = process.env.NODE_ENV
const originalDataProfile = process.env.GODDARD_DATA_PROFILE
const originalLocalAppData = process.env.LOCALAPPDATA
const originalXdgStateHome = process.env.XDG_STATE_HOME

afterEach(() => {
  restoreEnv("HOME", originalHome)
  restoreEnv("NODE_ENV", originalNodeEnv)
  restoreEnv("GODDARD_DATA_PROFILE", originalDataProfile)
  restoreEnv("LOCALAPPDATA", originalLocalAppData)
  restoreEnv("XDG_STATE_HOME", originalXdgStateHome)
})

test("getDatabasePath keeps the production path by default", () => {
  process.env.HOME = "/tmp/goddard-home"
  delete process.env.NODE_ENV
  delete process.env.GODDARD_DATA_PROFILE

  expect(getDatabasePath()).toBe(join("/tmp/goddard-home", ".goddard", "goddard.db"))
})

test("getDatabasePath isolates development data when the daemon data profile is set", () => {
  process.env.HOME = "/tmp/goddard-home"
  process.env.GODDARD_DATA_PROFILE = "development"

  expect(getDatabasePath()).toBe(join("/tmp/goddard-home", ".goddard", "development", "goddard.db"))
})

test("getDatabasePath isolates mock data when the daemon data profile is set", () => {
  process.env.HOME = "/tmp/goddard-home"
  process.env.GODDARD_DATA_PROFILE = "mock"

  expect(getDatabasePath()).toBe(join("/tmp/goddard-home", ".goddard", "mock", "goddard.db"))
})

test("getDatabasePath isolates development data for direct development runs", () => {
  process.env.HOME = "/tmp/goddard-home"
  process.env.NODE_ENV = "development"
  delete process.env.GODDARD_DATA_PROFILE

  expect(getDatabasePath()).toBe(join("/tmp/goddard-home", ".goddard", "development", "goddard.db"))
})

test("getAppStatePath stores app-owned state under the user directory", () => {
  process.env.HOME = "/tmp/goddard-home"

  expect(getAppStatePath()).toBe(join("/tmp/goddard-home", ".goddard", "user", "app-state.json"))
})

test("getGoddardTempLogDir stores process logs under a well-known temp directory", () => {
  expect(getGoddardTempLogDir()).toBe(join(tmpdir(), "goddard", "logs"))
})

test("getGoddardLogDatabasePath stores diagnostic logs under the OS log directory", () => {
  process.env.HOME = "/tmp/goddard-home"
  delete process.env.LOCALAPPDATA
  delete process.env.XDG_STATE_HOME

  if (process.platform === "darwin") {
    expect(getGoddardLogDir()).toBe(join("/tmp/goddard-home", "Library", "Logs", "Goddard"))
  } else if (process.platform === "win32") {
    expect(getGoddardLogDir()).toBe(
      join("/tmp/goddard-home", "AppData", "Local", "Goddard", "Logs"),
    )
  } else {
    expect(getGoddardLogDir()).toBe(join("/tmp/goddard-home", ".local", "state", "goddard", "log"))
  }

  expect(getGoddardLogDatabasePath()).toBe(join(getGoddardLogDir(), "logs.sqlite"))
})

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
