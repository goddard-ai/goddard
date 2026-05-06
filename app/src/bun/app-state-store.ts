import { getAppStatePath } from "@goddard-ai/paths/node"

import { APP_STATE_FILE_VERSION, AppStateFile, type AppStateSnapshot } from "~/shared/app-state.ts"
import { readJsonFile, writeJsonFile } from "./json-file.ts"

/** Reads the latest app-state snapshot from the Bun-host JSON file. */
export async function loadAppStateSnapshot() {
  const file = await readJsonFile(getAppStatePath(), AppStateFile)
  return file?.value ?? null
}

/** Atomically writes the latest app-state snapshot to the Bun-host JSON file. */
export async function writeAppStateSnapshot(snapshot: AppStateSnapshot) {
  await writeJsonFile(getAppStatePath(), {
    version: APP_STATE_FILE_VERSION,
    savedAt: Date.now(),
    value: snapshot,
  } satisfies AppStateFile)
  return snapshot
}
