import { z } from 'zod';

export interface PiAgentConfig {
  model: string;
  projectDir: string;
  maxTokensPerCycle?: number;
  [key: string]: any;
}

export interface CycleContext {
  cycleNumber: number;
  lastSummary?: string;
}

export interface CycleStrategy {
  nextPrompt(ctx: CycleContext): string;
}

export interface LoopConfig {
  agent: PiAgentConfig;
  strategy: CycleStrategy;
  rateLimits: {
    cycleDelay: string;        // '30m', '2h', '1d'
    maxTokensPerCycle: number;
    maxOpsPerMinute: number;
    maxCyclesBeforePause?: number;
  };
  metrics?: {
    prometheusPort?: number;
    enableLogging?: boolean;
  };
  systemd?: {
    restartSec?: number;
    nice?: number;
    user?: string;
    workingDir?: string;
    environment?: Record<string, string | undefined>;
  };
}

export interface LoopStatus {
  cycle: number;
  tokensUsed: number;
  uptime: number;
}

export interface TypedLoop<Config extends LoopConfig> {
  start: () => Promise<never>;
  status: LoopStatus;
}

export const configSchema = z.object({
  agent: z.object({
    model: z.string().min(1),
    projectDir: z.string().min(1),
    maxTokensPerCycle: z.number().int().positive().optional()
  }).passthrough(),
  strategy: z.custom<CycleStrategy>((val) => {
    return typeof val === 'object' && val !== null && 'nextPrompt' in val;
  }, "Strategy must have a nextPrompt method"),
  rateLimits: z.object({
    cycleDelay: z.string().min(1),
    maxTokensPerCycle: z.number().int().positive(),
    maxOpsPerMinute: z.number().int().positive(),
    maxCyclesBeforePause: z.number().int().positive().optional()
  }),
  metrics: z.object({
    prometheusPort: z.number().int().positive().optional(),
    enableLogging: z.boolean().optional()
  }).optional(),
  systemd: z.object({
    restartSec: z.number().int().positive().optional(),
    nice: z.number().int().optional(),
    user: z.string().optional(),
    workingDir: z.string().optional(),
    environment: z.record(z.string().optional()).optional()
  }).optional()
});
