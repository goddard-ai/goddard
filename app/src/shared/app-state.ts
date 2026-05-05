import { z } from "zod"

export const APP_STATE_STORAGE_KEY = "goddard.app.state.v1"
export const APP_STATE_RECORD_VERSION = 1

/** App-owned persisted Sigma snapshot transported between the webview and Bun host. */
export type AppStateSnapshot = {
  appearance: unknown
  navigation: unknown
  projectContext: unknown
  projectRegistry: unknown
  workbenchTabSet: unknown
}

/** Versioned app state record stored in the Bun-host app state kindstore. */
export const AppStateStorageRecord = z.strictObject({
  key: z.string().min(1),
  version: z.literal(APP_STATE_RECORD_VERSION),
  savedAt: z.number().int().nonnegative(),
  value: z.unknown(),
})

export type AppStateStorageRecord = z.output<typeof AppStateStorageRecord> & {
  id: `ast_${string}`
}
