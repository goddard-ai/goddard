const fs = require('fs');

// Add strategy to LoopStorage
let schema = fs.readFileSync('core/storage/src/db/schema.ts', 'utf8');
schema = schema.replace(
  'systemPrompt: text().notNull(),',
  'systemPrompt: text().notNull(),\n  strategy: text(),'
);
fs.writeFileSync('core/storage/src/db/schema.ts', schema);

// Re-do config change
let config = fs.readFileSync('core/config/src/index.ts', 'utf8');

const newConfigContent = `import { z } from "zod"
import * as acp from "@agentclientprotocol/sdk"

// ---------------------------------------------------------------------------
// configSchema (top-level)
// ---------------------------------------------------------------------------

/**
 * Zod schema for the full {@link GoddardLoopConfig}.
 */
export const configSchema = z
  .object({
    id: z.string().min(1),
    agent: z.string().min(1),
    systemPrompt: z.string().min(1),
    strategy: z.string().optional(),
    displayName: z.string().min(1),
    cwd: z.string().min(1),
    mcpServers: z.array(z.custom<acp.McpServer>()).default([]),
    gitRemote: z.string().default("origin"),

    rateLimits: z.object({
      /** Minimum pause between cycles. Accepts a human-readable duration string (e.g. \`"30m"\`, \`"2h"\`). */
      cycleDelay: z.string().min(1),
      /** Hard cap on tokens consumed in a single cycle. The loop throws if this is exceeded. */
      maxTokensPerCycle: z.number().int().positive(),
      /** Maximum agent operations (tool calls + messages) allowed per minute across all cycles. */
      maxOpsPerMinute: z.number().int().positive(),
      /** Pause the loop for 24 hours after this many cycles. Omit to run indefinitely. */
      maxCyclesBeforePause: z.number().int().positive().optional(),
    }),
    retries: z
      .object({
        /** Maximum number of send attempts per cycle before the error is re-thrown. Defaults to \`1\` (no retry). */
        maxAttempts: z.number().int().positive().optional(),
        /** Delay before the first retry, in milliseconds. Defaults to \`1000\`. */
        initialDelayMs: z.number().int().nonnegative().optional(),
        /** Upper bound on the computed backoff delay, in milliseconds. Defaults to \`30000\`. */
        maxDelayMs: z.number().int().positive().optional(),
        /** Exponential backoff multiplier applied after each failed attempt. Defaults to \`2\`. */
        backoffFactor: z.number().positive().optional(),
        /**
         * Random jitter applied to each retry delay as a fraction of the computed delay.
         * \`0.2\` means ±20 %. Defaults to \`0.2\`.
         */
        jitterRatio: z.number().min(0).max(1).optional(),
        /**
         * Predicate that decides whether a given error is retryable.
         * Return \`true\` to retry, \`false\` to re-throw immediately.
         * Defaults to always retrying.
         */
        retryableErrors: z
          .custom<
            (
              error: unknown,
              context: { cycle: number; attempt: number; maxAttempts: number },
            ) => boolean
          >(
            (val) => val === undefined || typeof val === "function",
            "retries.retryableErrors must be a function",
          )
          .optional(),
      })
      .optional(),
    metrics: z
      .object({
        /** Port on which to expose a Prometheus \`/metrics\` endpoint. Omit to disable. */
        prometheusPort: z.number().int().positive().optional(),
        /** Emit structured log lines for every cycle. Defaults to \`true\`. */
        enableLogging: z.boolean().default(true),
      })
      .default({ enableLogging: true }),
    systemd: z
      .object({
        /** Seconds systemd should wait before restarting the service after a crash. */
        restartSec: z.number().int().positive().optional(),
        /** \`nice\` priority for the service process (\`-20\` highest, \`19\` lowest). */
        nice: z.number().int().optional(),
        /** Unix user the service runs as. Defaults to the invoking user. */
        user: z.string().optional(),
        /** Override the systemd \`WorkingDirectory\`. Defaults to the project directory. */
        workingDir: z.string().optional(),
        /** Additional environment variables injected into the service unit. */
        environment: z.record(z.string(), z.string().optional()).optional(),
      })
      .optional(),
  })
  .superRefine((config, ctx) => {
    if (
      config.retries?.initialDelayMs !== undefined &&
      config.retries?.maxDelayMs !== undefined &&
      config.retries.maxDelayMs < config.retries.initialDelayMs
    ) {
      ctx.addIssue({
        code: "custom",
        path: ["retries", "maxDelayMs"],
        message: \`retries.maxDelayMs (\${config.retries.maxDelayMs}) must be >= retries.initialDelayMs (\${config.retries.initialDelayMs}).\`,
      })
    }
  })

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
`;

fs.writeFileSync('core/config/src/index.ts', newConfigContent);
