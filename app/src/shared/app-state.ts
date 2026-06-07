import { z } from "zod"

export const APP_STATE_FILE_VERSION = 1

export const WindowFrame = z.strictObject({
  x: z.number().finite(),
  y: z.number().finite(),
  width: z.number().finite().positive(),
  height: z.number().finite().positive(),
})

export const WindowLayoutSnapshot = z.strictObject({
  mainWindow: z.strictObject({
    frame: WindowFrame,
  }),
})

/** Versioned app state JSON file stored by the Bun host. */
export const AppStateFile = z.strictObject({
  version: z.literal(APP_STATE_FILE_VERSION),
  savedAt: z.number().int().nonnegative(),
  value: z.record(z.string(), z.any()),
})

export type AppStateSnapshot = z.output<typeof AppStateFile>["value"]
export type AppStateFile = z.output<typeof AppStateFile>
export type WindowFrame = z.output<typeof WindowFrame>
export type WindowLayoutSnapshot = z.output<typeof WindowLayoutSnapshot>
