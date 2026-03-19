import { z } from "zod"
import {
  PiAgentConfig as SharedPiAgentConfig,
  ThinkingLevel as SharedThinkingLevel,
} from "./common/agent-config.js"

export { SharedPiAgentConfig as PiAgentConfig, SharedThinkingLevel as ThinkingLevel }

/**
 * File-based loop settings are no longer modeled here.
 * The config file remains a passthrough object so callers can continue using
 * `defineConfig`, while runtime loop settings are sourced elsewhere.
 */
export const GoddardLoopConfig = z.object({}).passthrough()

export type GoddardLoopConfig = z.infer<typeof GoddardLoopConfig>
