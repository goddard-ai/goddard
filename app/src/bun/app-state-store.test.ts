import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { getAppStatePath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "vitest"

import type { AppStateSnapshot } from "~/shared/app-state.ts"
import {
  loadAppStateSnapshot,
  updateAppStateSnapshotSync,
  writeAppStateSnapshot,
} from "./app-state-store.ts"

const originalHome = process.env.HOME
const tempHomes: string[] = []

afterEach(async () => {
  restoreEnv("HOME", originalHome)

  await Promise.all(tempHomes.splice(0).map((home) => rm(home, { force: true, recursive: true })))
})

test("app state persistence returns null when the app state file is absent", async () => {
  process.env.HOME = await createTempHome()

  await expect(loadAppStateSnapshot()).resolves.toBeNull()
})

test("app state persistence writes and reads the Goddard user app state file", async () => {
  const home = await createTempHome()
  process.env.HOME = home
  const firstSnapshot = createSnapshot("inbox")
  const secondSnapshot = createSnapshot("sessions")

  await expect(writeAppStateSnapshot(firstSnapshot)).resolves.toEqual(firstSnapshot)
  await expect(writeAppStateSnapshot(secondSnapshot)).resolves.toEqual(secondSnapshot)

  await expect(loadAppStateSnapshot()).resolves.toEqual(secondSnapshot)
  expect(getAppStatePath()).toBe(join(home, ".goddard", "user", "app-state.json"))

  const source = await readFile(getAppStatePath(), "utf8")
  const file = JSON.parse(source)

  expect(source.endsWith("\n")).toBe(true)
  expect(file.version).toBe(1)
  expect(typeof file.savedAt).toBe("number")
  expect(file.value).toEqual(secondSnapshot)
})

test("app state persistence preserves host-owned window layout during UI snapshot writes", async () => {
  process.env.HOME = await createTempHome()

  await writeAppStateSnapshot({
    ...createSnapshot("inbox"),
    windowLayout: {
      mainWindow: {
        frame: {
          x: 20,
          y: 40,
          width: 1200,
          height: 800,
        },
      },
    },
  })

  await writeAppStateSnapshot(createSnapshot("sessions"))

  await expect(loadAppStateSnapshot()).resolves.toEqual({
    ...createSnapshot("sessions"),
    windowLayout: {
      mainWindow: {
        frame: {
          x: 20,
          y: 40,
          width: 1200,
          height: 800,
        },
      },
    },
  })
})

test("app state persistence supports sync updates for shutdown handlers", async () => {
  process.env.HOME = await createTempHome()
  await writeAppStateSnapshot(createSnapshot("inbox"))

  const updatedSnapshot = updateAppStateSnapshotSync((snapshot) => ({
    ...snapshot,
    windowLayout: {
      mainWindow: {
        frame: {
          x: 60,
          y: 80,
          width: 1280,
          height: 820,
        },
      },
    },
  }))

  await expect(loadAppStateSnapshot()).resolves.toEqual(updatedSnapshot)
})

test("app state persistence rejects invalid app state files", async () => {
  process.env.HOME = await createTempHome()
  await mkdir(dirname(getAppStatePath()), { recursive: true })
  await writeFile(
    getAppStatePath(),
    JSON.stringify({
      version: 1,
      savedAt: Date.now(),
    }),
    "utf8",
  )

  await expect(loadAppStateSnapshot()).rejects.toThrow()
})

function createSnapshot(selectedKind: string): AppStateSnapshot {
  return {
    appearance: {
      mode: "system",
      highContrast: false,
    },
    mainTab: {
      selectedKind,
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
