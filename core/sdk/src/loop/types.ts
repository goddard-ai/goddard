import type { Model } from "@goddard-ai/config"
import {
  type LoopConfig,
  type ResolvedLoopRateLimits,
  type ResolvedLoopRetries,
} from "@goddard-ai/schema/config"

export type {
  LoopConfig as GoddardLoopConfigDocument,
  ResolvedLoopRateLimits as GoddardLoopRateLimitsConfig,
  ResolvedLoopRetries as GoddardLoopRetriesConfig,
  Model,
  ResolvedLoopRateLimits,
  ResolvedLoopRetries,
}
