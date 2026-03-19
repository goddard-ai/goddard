import type { Model } from "@goddard-ai/config"
import {
  LoopConfig,
  ResolvedLoopRateLimits,
  ResolvedLoopRetries,
  type GoddardLoopConfigDocument,
  type GoddardLoopRateLimitsConfig,
  type GoddardLoopRetriesConfig,
} from "@goddard-ai/schema/config"

export {
  LoopConfig as loopConfigSchema,
  ResolvedLoopRateLimits as resolvedLoopRateLimitsSchema,
  ResolvedLoopRetries as resolvedLoopRetriesSchema,
}
export type {
  GoddardLoopConfigDocument,
  GoddardLoopRateLimitsConfig,
  GoddardLoopRetriesConfig,
  Model,
}
