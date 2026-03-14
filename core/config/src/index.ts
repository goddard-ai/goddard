import { z } from "zod"

// ---------------------------------------------------------------------------
// configSchema (top-level)
// ---------------------------------------------------------------------------

/**
 * Zod schema for the full {@link GoddardLoopConfig}.
 * Currently an empty object as all configuration is passed directly at runtime.
 */
export const configSchema = z.object({}).passthrough()

/**
 * Full configuration object for a Goddard agent loop.
 */
export type GoddardLoopConfig = z.infer<typeof configSchema>

// ---------------------------------------------------------------------------
// defineConfig
// ---------------------------------------------------------------------------

/**
 * Identity helper that types your config object as {@link GoddardLoopConfig}.
 */
export function defineConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config
}
