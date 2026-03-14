import type { SessionParams } from "./session-server.js"

export interface LoopContext {
  cycleNumber: number;
  lastSummary?: string;
}

export type LoopStrategy = {
  nextPrompt(ctx: LoopContext): string;
}

export interface AgentLoopParams {
  session: SessionParams & { oneShot?: undefined };
  strategy: LoopStrategy;
  rateLimits?: {
    cycleDelay?: string;
    maxTokensPerCycle?: number;
    maxOpsPerMinute?: number;
    maxCyclesBeforePause?: number;
  };
}
