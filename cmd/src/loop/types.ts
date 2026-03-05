import type {
  ThinkingLevel,
  PiAgentConfig,
  CycleContext,
  CycleStrategy,
  GoddardLoopConfig,
  Model
} from "@goddard-ai/config";
import { configSchema } from "@goddard-ai/config";

export interface LoopStatus {
  cycle: number;
  tokensUsed: number;
  uptime: number;
}

export interface GoddardLoop {
  start: () => Promise<void>;
  status: LoopStatus;
}

export { configSchema };
export type { ThinkingLevel, PiAgentConfig, CycleContext, CycleStrategy, GoddardLoopConfig, Model };
