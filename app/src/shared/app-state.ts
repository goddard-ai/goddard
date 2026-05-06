import { z } from "zod"

export const APP_STATE_FILE_VERSION = 1

const RequiredUnknown = z.custom<unknown>((value) => value !== undefined)

/** App-owned persisted Sigma snapshot transported between the webview and Bun host. */
export type AppStateSnapshot = {
  appearance: unknown
  navigation: unknown
  projectContext: unknown
  projectRegistry: unknown
  workbenchTabSet: unknown
}

/** Versioned app state JSON file stored by the Bun host. */
export const AppStateFile = z.strictObject({
  version: z.literal(APP_STATE_FILE_VERSION),
  savedAt: z.number().int().nonnegative(),
  value: z.strictObject({
    appearance: RequiredUnknown,
    navigation: RequiredUnknown,
    projectContext: RequiredUnknown,
    projectRegistry: RequiredUnknown,
    workbenchTabSet: RequiredUnknown,
  }),
})

export type AppStateFile = z.output<typeof AppStateFile>
