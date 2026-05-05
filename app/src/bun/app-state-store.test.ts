import { existsSync } from "node:fs"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { getAppStateDatabasePath } from "@goddard-ai/paths/node"
import { afterEach, beforeEach, expect, test } from "bun:test"

import type { AppStateSnapshot } from "~/shared/app-state.ts"
import {
  appStateDb,
  closeAppStateDb,
  loadAppStateSnapshot,
  resetAppStateDb,
  writeAppStateSnapshot,
} from "./app-state-store.ts"

const originalHome = process.env.HOME
const tempHomes: string[] = []

beforeEach(() => {
  resetAppStateDb({ filename: ":memory:" })
})

afterEach(async () => {
  closeAppStateDb()
  restoreEnv("HOME", originalHome)

  await Promise.all(tempHomes.splice(0).map((home) => rm(home, { force: true, recursive: true })))
})

test("app state kindstore returns null before a snapshot is saved", () => {
  expect(loadAppStateSnapshot()).toBeNull()
})

test("app state kindstore replaces the latest snapshot by stable key", () => {
  const firstSnapshot = createSnapshot("inbox")
  const secondSnapshot = createSnapshot("sessions")

  expect(writeAppStateSnapshot(firstSnapshot)).toEqual(firstSnapshot)
  expect(writeAppStateSnapshot(secondSnapshot)).toEqual(secondSnapshot)

  expect(loadAppStateSnapshot()).toEqual(secondSnapshot)
  expect(appStateDb.appStateRecords.findMany()).toHaveLength(1)
})

test("app state kindstore uses the app-owned database path", async () => {
  const home = await createTempHome()
  process.env.HOME = home
  resetAppStateDb()

  writeAppStateSnapshot(createSnapshot("sessions"))

  expect(getAppStateDatabasePath()).toBe(join(home, ".goddard", "user", "app-state.db"))
  expect(existsSync(getAppStateDatabasePath())).toBe(true)
})

function createSnapshot(selectedNavId: string): AppStateSnapshot {
  return {
    appearance: {
      mode: "system",
      highContrast: false,
    },
    navigation: {
      selectedNavId,
    },
    projectContext: {
      selectedProjectPath: null,
    },
    projectRegistry: {
      projects: [],
    },
    workbenchTabSet: {
      tabs: [],
    },
  }
}

async function createTempHome() {
  const home = await mkdtemp(join(tmpdir(), "goddard-app-state-"))
  tempHomes.push(home)
  return home
}

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
