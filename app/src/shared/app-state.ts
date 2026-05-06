import { z } from "zod"

export const APP_STATE_FILE_VERSION = 1

const RequiredUnknown = z.custom<unknown>((value) => value !== undefined)

/** App-owned persisted Sigma snapshot transported between the webview and Bun host. */
export const AppStateSnapshot = z.strictObject({
  appearance: RequiredUnknown,
  navigation: RequiredUnknown,
  projectContext: RequiredUnknown,
  projectRegistry: RequiredUnknown,
  workbenchTabSet: RequiredUnknown,
})

export type AppStateSnapshot = z.output<typeof AppStateSnapshot>

/** Versioned app state JSON file stored by the Bun host. */
export const AppStateFile = z.strictObject({
  version: z.literal(APP_STATE_FILE_VERSION),
  savedAt: z.number().int().nonnegative(),
  value: AppStateSnapshot,
})

export type AppStateFile = z.output<typeof AppStateFile>
