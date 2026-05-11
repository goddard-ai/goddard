import { z } from "zod"

export const APP_STATE_FILE_VERSION = 1

/** Versioned app state JSON file stored by the Bun host. */
export const AppStateFile = z.strictObject({
  version: z.literal(APP_STATE_FILE_VERSION),
  savedAt: z.number().int().nonnegative(),
  value: z.record(z.string(), z.any()),
})

export type AppStateSnapshot = z.output<typeof AppStateFile>["value"]
export type AppStateFile = z.output<typeof AppStateFile>
