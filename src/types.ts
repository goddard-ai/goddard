import { z } from 'zod';

export type ThinkingLevel = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh';

export interface PiAgentConfig {
  model: string;
  projectDir: string;
  thinkingLevel?: ThinkingLevel;
  agentDir?: string;
  [key: string]: any;
}

export interface CycleContext {
  cycleNumber: number;
  lastSummary?: string;
}

export interface CycleStrategy {
  nextPrompt(ctx: CycleContext): string;
}

/**
 * Configuration for a continuous execution loop.
 * This defines how the agent operates over time, including pacing, retries, and system integration.
 */
export interface LoopConfig {
  /**
   * The underlying Pi Agent configuration.
   */
  agent: PiAgentConfig;
  /**
   * Defines how to generate the prompt for the next cycle.
   */
  strategy: CycleStrategy;
  /**
   * Controls the pacing of the loop to prevent runaway execution or exceeding API quotas.
   */
  rateLimits: {
    /** Time to wait between cycles (e.g., '30m', '2h', '1d'). */
    cycleDelay: string;
    /** Maximum number of tokens allowed to be consumed in a single cycle. */
    maxTokensPerCycle: number;
    /** Maximum operations (like tool calls) permitted per minute. */
    maxOpsPerMinute: number;
    /** Pause the loop after this many cycles to await human intervention or reset. */
    maxCyclesBeforePause?: number;
  };
  /**
   * Configuration for recovering from transient errors.
   */
  retries?: {
    /** Maximum number of retry attempts for a failed cycle. */
    maxAttempts?: number;
    /** Initial delay before the first retry attempt. */
    initialDelayMs?: number;
    /** Maximum delay between retries, capping the exponential backoff. */
    maxDelayMs?: number;
    /** Multiplier applied to the delay after each retry. */
    backoffFactor?: number;
    /** Adds randomness to the delay to prevent thundering herd problems (0-1). */
    jitterRatio?: number;
    /** Custom logic to determine if an error should trigger a retry. */
    retryableErrors?: (error: unknown, context: { cycle: number; attempt: number; maxAttempts: number }) => boolean;
  };
  /**
   * Observability and logging configuration.
   */
  metrics?: {
    /** Port to expose Prometheus metrics. */
    prometheusPort?: number;
    /** Whether to enable detailed loop logging. */
    enableLogging?: boolean;
  };
  /**
   * Configuration for running the loop as a systemd service.
   */
  systemd?: {
    /** Time to sleep before restarting a service (RestartSec). */
    restartSec?: number;
    /** Process scheduling priority (-20 to 19). */
    nice?: number;
    /** User to run the service as. */
    user?: string;
    /** Working directory for the service. */
    workingDir?: string;
    /** Environment variables to pass to the service. */
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
    thinkingLevel: z.enum(['off', 'minimal', 'low', 'medium', 'high', 'xhigh']).optional(),
    agentDir: z.string().optional()
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
    enableLogging: z.boolean().default(true)
  }).default({ enableLogging: true }),
  systemd: z.object({
    restartSec: z.number().int().positive().optional(),
    nice: z.number().int().optional(),
    user: z.string().optional(),
    workingDir: z.string().optional(),
    environment: z.record(z.string().optional()).optional()
  }).optional()
}).superRefine((config, ctx) => {
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
