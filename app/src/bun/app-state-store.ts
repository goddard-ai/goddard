import { randomUUID } from "node:crypto"
import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs"
import { dirname } from "node:path"
import { getAppStatePath } from "@goddard-ai/paths/node"

import { APP_STATE_FILE_VERSION, AppStateFile, type AppStateSnapshot } from "~/shared/app-state.ts"
import { readJsonFile, writeJsonFile } from "./json-file.ts"

const HOST_OWNED_APP_STATE_KEYS = ["windowLayout"] as const

let appStateWriteQueue = Promise.resolve()

/** Reads the latest app-state snapshot from the Bun-host JSON file. */
export async function loadAppStateSnapshot() {
  const file = await readJsonFile(getAppStatePath(), AppStateFile)
  return file?.value ?? null
}

/** Atomically writes the latest app-state snapshot to the Bun-host JSON file. */
export async function writeAppStateSnapshot(snapshot: AppStateSnapshot) {
  return await updateAppStateSnapshot((currentSnapshot) => {
    const nextSnapshot = { ...snapshot }

    for (const key of HOST_OWNED_APP_STATE_KEYS) {
      if (!(key in nextSnapshot) && key in currentSnapshot) {
        nextSnapshot[key] = currentSnapshot[key]
      }
    }

    return nextSnapshot
  })
}

/** Updates the app-state snapshot while preserving other writers in this Bun process. */
export async function updateAppStateSnapshot(
  updateSnapshot: (snapshot: AppStateSnapshot) => AppStateSnapshot,
) {
  const write = appStateWriteQueue.then(async () => {
    const currentSnapshot = (await loadAppStateSnapshot()) ?? {}
    const nextSnapshot = updateSnapshot(currentSnapshot)
    const file = AppStateFile.parse({
      version: APP_STATE_FILE_VERSION,
      savedAt: Date.now(),
      value: nextSnapshot,
    })

    await writeJsonFile(getAppStatePath(), file)
    return nextSnapshot
  })

  appStateWriteQueue = write.then(
    () => {},
    () => {},
  )

  return await write
}

/** Synchronously updates app state from shutdown handlers that cannot await pending writes. */
export function updateAppStateSnapshotSync(
  updateSnapshot: (snapshot: AppStateSnapshot) => AppStateSnapshot,
) {
  const filePath = getAppStatePath()
  const currentSnapshot = readAppStateSnapshotSync(filePath)
  const nextSnapshot = updateSnapshot(currentSnapshot)
  const file = AppStateFile.parse({
    version: APP_STATE_FILE_VERSION,
    savedAt: Date.now(),
    value: nextSnapshot,
  })

  writeAppStateFileSync(filePath, file)
  return nextSnapshot
}

function readAppStateSnapshotSync(filePath: string): AppStateSnapshot {
  try {
    const source = readFileSync(filePath, "utf8")
    return AppStateFile.parse(JSON.parse(source)).value
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      return {}
    }

    throw error
  }
}

function writeAppStateFileSync(filePath: string, value: AppStateFile) {
  const temporaryPath = `${filePath}.${process.pid}.${randomUUID()}.tmp`

  mkdirSync(dirname(filePath), { recursive: true })

  try {
    writeFileSync(temporaryPath, `${JSON.stringify(value, null, 2)}\n`, "utf8")
    renameSync(temporaryPath, filePath)
  } catch (error) {
    rmSync(temporaryPath, { force: true })
    throw error
  }
}
