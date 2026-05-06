import { randomUUID } from "node:crypto"
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getAppStatePath } from "@goddard-ai/paths/node"

import { APP_STATE_FILE_VERSION, AppStateFile, type AppStateSnapshot } from "~/shared/app-state.ts"

function isNodeErrorCode(error: unknown, code: string) {
  return error instanceof Error && "code" in error && error.code === code
}

/** Reads the latest app-state snapshot from the Bun-host JSON file. */
export async function loadAppStateSnapshot() {
  let source: string

  try {
    source = await readFile(getAppStatePath(), "utf8")
  } catch (error) {
    if (isNodeErrorCode(error, "ENOENT")) {
      return null
    }

    throw error
  }

  return AppStateFile.parse(JSON.parse(source)).value as AppStateSnapshot
}

/** Atomically writes the latest app-state snapshot to the Bun-host JSON file. */
export async function writeAppStateSnapshot(snapshot: AppStateSnapshot) {
  const appStatePath = getAppStatePath()
  const temporaryPath = `${appStatePath}.${process.pid}.${randomUUID()}.tmp`

  await mkdir(dirname(appStatePath), { recursive: true })

  try {
    await writeFile(
      temporaryPath,
      `${JSON.stringify(
        {
          version: APP_STATE_FILE_VERSION,
          savedAt: Date.now(),
          value: snapshot,
        } satisfies AppStateFile,
        null,
        2,
      )}\n`,
      "utf8",
    )
    await rename(temporaryPath, appStatePath)
  } catch (error) {
    await rm(temporaryPath, { force: true }).catch(() => {})
    throw error
  }

  return snapshot
}
