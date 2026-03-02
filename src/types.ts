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
  retries?: {
    maxAttempts?: number;
    initialDelayMs?: number;
    maxDelayMs?: number;
    backoffFactor?: number;
    jitterRatio?: number;
    retryableErrors?: (error: unknown, context: { cycle: number; attempt: number; maxAttempts: number }) => boolean;
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
  start: () => Promise<void>;
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
  retries: z.object({
    maxAttempts: z.number().int().positive().optional(),
    initialDelayMs: z.number().int().nonnegative().optional(),
    maxDelayMs: z.number().int().positive().optional(),
    backoffFactor: z.number().positive().optional(),
    jitterRatio: z.number().min(0).max(1).optional(),
    retryableErrors: z.custom<(error: unknown, context: { cycle: number; attempt: number; maxAttempts: number }) => boolean>(
      (val) => val === undefined || typeof val === 'function',
      'retries.retryableErrors must be a function'
    ).optional()
  }).optional(),
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
}).superRefine((config, ctx) => {
  const agentLimit = config.agent.maxTokensPerCycle;
  const rateLimit = config.rateLimits.maxTokensPerCycle;

  if (agentLimit !== undefined && agentLimit !== rateLimit) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      path: ['agent', 'maxTokensPerCycle'],
      message: `agent.maxTokensPerCycle (${agentLimit}) must match rateLimits.maxTokensPerCycle (${rateLimit}) when both are set.`
    });
  }

  if (
    config.retries?.initialDelayMs !== undefined &&
    config.retries?.maxDelayMs !== undefined &&
    config.retries.maxDelayMs < config.retries.initialDelayMs
  ) {
    ctx.addIssue({
      code: z.ZodIssueCode.custom,
      path: ['retries', 'maxDelayMs'],
      message: `retries.maxDelayMs (${config.retries.maxDelayMs}) must be >= retries.initialDelayMs (${config.retries.initialDelayMs}).`
    });
  }
});
