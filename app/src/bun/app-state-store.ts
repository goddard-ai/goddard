import { mkdirSync, rmSync } from "node:fs"
import { dirname } from "node:path"
import { getAppStateDatabasePath } from "@goddard-ai/paths/node"
import { kind, kindstore, UnrecoverableStoreOpenError, type DatabaseOptions } from "kindstore"

import {
  APP_STATE_RECORD_VERSION,
  APP_STATE_STORAGE_KEY,
  AppStateStorageRecord,
  type AppStateSnapshot,
} from "~/shared/app-state.ts"

type StoreConnectionOptions = {
  filename: string
  databaseOptions?: DatabaseOptions
}

const schema = {
  appStateRecords: kind("ast", AppStateStorageRecord).index("key", { unique: true }),
}

function createStore(options: StoreConnectionOptions) {
  if (options.filename !== ":memory:") {
    mkdirSync(dirname(options.filename), { recursive: true })
  }

  return kindstore({
    filename: options.filename,
    databaseOptions: options.databaseOptions,
    schema,
  })
}

function removeDatabaseArtifacts(filename: string) {
  for (const suffix of ["", "-shm", "-wal"]) {
    rmSync(`${filename}${suffix}`, { force: true })
  }
}

function openStore(connection: StoreConnectionOptions) {
  try {
    return createStore(connection)
  } catch (error) {
    if (connection.filename === ":memory:" || !(error instanceof UnrecoverableStoreOpenError)) {
      throw error
    }

    removeDatabaseArtifacts(connection.filename)
    return createStore(connection)
  }
}

/**
 * Shared app-state kindstore handle owned by the Electrobun Bun host.
 * Tests that override HOME should call `resetAppStateDb()` after changing it.
 */
export let appStateDb =
  process.env.NODE_ENV !== "test" ? openStore({ filename: getAppStateDatabasePath() }) : null!

/** Recreates the shared app-state kindstore handle with an optional explicit connection. */
export function resetAppStateDb(
  connection: StoreConnectionOptions = { filename: getAppStateDatabasePath() },
) {
  appStateDb?.close()
  appStateDb = openStore(connection)
  return appStateDb
}

/** Closes the shared app-state kindstore handle. */
export function closeAppStateDb() {
  appStateDb?.close()
  appStateDb = null!
}

/** Reads the latest app-state snapshot from the Bun-host app-state kindstore. */
export function loadAppStateSnapshot() {
  const record =
    appStateDb.appStateRecords.first({
      where: { key: APP_STATE_STORAGE_KEY },
    }) ?? null

  return (record?.value ?? null) as AppStateSnapshot | null
}

/** Replaces the app-state snapshot in the Bun-host app-state kindstore. */
export function writeAppStateSnapshot(snapshot: AppStateSnapshot) {
  const record = appStateDb.appStateRecords.putByUnique(
    { key: APP_STATE_STORAGE_KEY },
    {
      key: APP_STATE_STORAGE_KEY,
      version: APP_STATE_RECORD_VERSION,
      savedAt: Date.now(),
      value: snapshot,
    },
  )

  return record.value as AppStateSnapshot
}
