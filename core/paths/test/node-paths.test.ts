import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { getDatabasePath } from "../src/node/index.ts"

const originalHome = process.env.HOME
const originalNodeEnv = process.env.NODE_ENV
const originalDataProfile = process.env.GODDARD_DATA_PROFILE

afterEach(() => {
  restoreEnv("HOME", originalHome)
  restoreEnv("NODE_ENV", originalNodeEnv)
  restoreEnv("GODDARD_DATA_PROFILE", originalDataProfile)
})

test("getDatabasePath keeps the production path by default", () => {
  process.env.HOME = "/tmp/goddard-home"
  delete process.env.NODE_ENV
  delete process.env.GODDARD_DATA_PROFILE

  expect(getDatabasePath()).toBe("/tmp/goddard-home/.goddard/goddard.db")
})

test("getDatabasePath isolates development data when the daemon data profile is set", () => {
  process.env.HOME = "/tmp/goddard-home"
  process.env.GODDARD_DATA_PROFILE = "development"

  expect(getDatabasePath()).toBe(join("/tmp/goddard-home", ".goddard", "development", "goddard.db"))
})

test("getDatabasePath isolates development data for direct development runs", () => {
  process.env.HOME = "/tmp/goddard-home"
  process.env.NODE_ENV = "development"
  delete process.env.GODDARD_DATA_PROFILE

  expect(getDatabasePath()).toBe(join("/tmp/goddard-home", ".goddard", "development", "goddard.db"))
})

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
