import { z } from "zod";

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/**
 * Well-known model identifiers, organised by provider.
 *
 * Use these constants in your config file instead of raw strings to get
 * autocomplete and catch typos at author-time.
 *
 * @example
 * ```ts
 * import { Models, defineConfig } from "@goddard-ai/config";
 *
 * export default defineConfig({
 *   agent: { model: Models.Anthropic.ClaudeSonnet45, projectDir: "./" },
 *   // …
 * });
 * ```
 */
export const Models = {
  /** Anthropic Claude model family. */
  Anthropic: {
    /** `anthropic/claude-3-7-sonnet-20250219` */
    Claude37Sonnet: "anthropic/claude-3-7-sonnet-20250219",
    /** `anthropic/claude-sonnet-4-5` */
    ClaudeSonnet45: "anthropic/claude-sonnet-4-5",
    /** `anthropic/claude-sonnet-4-6` */
    ClaudeSonnet46: "anthropic/claude-sonnet-4-6",
    /** `anthropic/claude-opus-4-6` */
    ClaudeOpus46: "anthropic/claude-opus-4-6",
  },
  /** OpenAI model family. */
  OpenAi: {
    /** `openai/o3-mini` */
    O3Mini: "openai/o3-mini",
    /** `openai/o3-pro` */
    O3Pro: "openai/o3-pro",
    /** `openai/gpt-5-codex` */
    Gpt5Codex: "openai/gpt-5-codex",
    /** `openai/gpt-5.1-codex` */
    Gpt51Codex: "openai/gpt-5.1-codex",
    /** `openai/gpt-5.2-codex` */
    Gpt52Codex: "openai/gpt-5.2-codex",
    /** `openai/gpt-5.3-codex` */
    Gpt53Codex: "openai/gpt-5.3-codex",
  },
} as const;

type _ValueOf<T> = T[keyof T];

/**
 * A model identifier string.
 *
 * The type is a loose literal union: all values from {@link Models} are
 * listed as literals so IDEs offer autocomplete, but any arbitrary string is
 * also accepted so you can reference provider-specific models that are not
 * yet in the constant list.
 *
 * @example
 * ```ts
 * const m: Model = Models.Anthropic.ClaudeSonnet45; // autocomplete ✓
 * const m: Model = "anthropic/claude-custom";        // open-ended ✓
 * ```
 */
export type Model =
  | _ValueOf<typeof Models.Anthropic>
  | _ValueOf<typeof Models.OpenAi>
  | (string & {});

// ---------------------------------------------------------------------------
// Primitive schemas
// ---------------------------------------------------------------------------

const thinkingLevelSchema = z.enum(["off", "minimal", "low", "medium", "high", "xhigh"]);

/**
 * Controls how much extended thinking budget the agent receives per turn.
 *
 * - `"off"` — no thinking tokens allocated.
 * - `"minimal"` / `"low"` / `"medium"` / `"high"` / `"xhigh"` — progressively
 *   larger budgets. Higher levels improve reasoning quality but increase
 *   latency and token cost.
 */
export type ThinkingLevel = z.infer<typeof thinkingLevelSchema>;

// ---------------------------------------------------------------------------
// CycleContext
// ---------------------------------------------------------------------------

const cycleContextSchema = z.object({
  cycleNumber: z.number(),
  lastSummary: z.string().optional(),
});

/**
 * Snapshot of loop state passed to {@link CycleStrategy.nextPrompt} at the
 * start of every cycle.
 */
export type CycleContext = z.infer<typeof cycleContextSchema>;

// ---------------------------------------------------------------------------
// CycleStrategy
//
// Functions cannot be expressed as first-class zod schemas, so we define the
// shape as a private type and pass it as the generic to z.custom<>. The
// exported CycleStrategy is still derived via z.infer so consumers always get
// the type from the schema rather than from a separate hand-written interface.
// ---------------------------------------------------------------------------

type _CycleStrategyShape = {
  nextPrompt(ctx: CycleContext): string;
};

const cycleStrategySchema = z.custom<_CycleStrategyShape>(
  (val) =>
    typeof val === "object" &&
    val !== null &&
    typeof (val as _CycleStrategyShape).nextPrompt === "function",
  "Strategy must have a nextPrompt method"
);

/**
 * Determines the prompt sent to the agent at the start of each cycle.
 *
 * Implement `nextPrompt` to encode your task description, any
 * cycle-to-cycle state you want the agent to recall, and your
 * completion/stop criteria.
 *
 * @example
 * ```ts
 * const strategy: CycleStrategy = {
 *   nextPrompt({ cycleNumber, lastSummary }) {
 *     return `Cycle ${cycleNumber}. Previous: ${lastSummary ?? "none"}. Continue.`;
 *   },
 * };
 * ```
 */
export type CycleStrategy = z.infer<typeof cycleStrategySchema>;

// ---------------------------------------------------------------------------
// Agent sub-schema
// ---------------------------------------------------------------------------

const agentSchema = z
  .object({
    // Runtime: any non-empty string is valid. Type-level: loose literal union
    // for autocomplete. Cast keeps both without a runtime transform.
    model: z.string().min(1) as z.ZodType<Model>,
    projectDir: z.string().min(1),
    thinkingLevel: thinkingLevelSchema.optional(),
    agentDir: z.string().optional(),
  })
  .passthrough();

/**
 * Configuration for the underlying pi coding agent.
 *
 * @property model - Model identifier. Use {@link Models} for autocomplete or
 *   pass a raw `"provider/modelId"` string.
 * @property projectDir - Working directory the agent operates in. Relative
 *   paths are resolved from the process cwd at loop start.
 * @property thinkingLevel - Optional extended-thinking budget. Defaults to
 *   the model's built-in default when omitted.
 * @property agentDir - Override for the pi agent config directory (the folder
 *   that contains `auth.json`, `models.json`, system prompts, etc.). Defaults
 *   to `~/.pi/agent` when omitted.
 *
 * Additional properties are forwarded to the agent session unchanged
 * (`passthrough`), so you can supply provider-specific options without
 * needing a type cast.
 */
export type PiAgentConfig = z.infer<typeof agentSchema>;

// ---------------------------------------------------------------------------
// configSchema (top-level)
// ---------------------------------------------------------------------------

/**
 * Zod schema for the full {@link GoddardLoopConfig}.
 *
 * Use this to validate a config object at runtime before handing it to
 * `createLoop`, or to build tooling that inspects the config shape.
 *
 * @example
 * ```ts
 * import { configSchema } from "@goddard-ai/config";
 *
 * const validated = configSchema.parse(rawConfig);
 * ```
 */
export const configSchema = z
  .object({
    agent: agentSchema,
    strategy: cycleStrategySchema,
    rateLimits: z.object({
      /** Minimum pause between cycles. Accepts a human-readable duration string (e.g. `"30m"`, `"2h"`). */
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
        /** Maximum number of send attempts per cycle before the error is re-thrown. Defaults to `1` (no retry). */
        maxAttempts: z.number().int().positive().optional(),
        /** Delay before the first retry, in milliseconds. Defaults to `1000`. */
        initialDelayMs: z.number().int().nonnegative().optional(),
        /** Upper bound on the computed backoff delay, in milliseconds. Defaults to `30000`. */
        maxDelayMs: z.number().int().positive().optional(),
        /** Exponential backoff multiplier applied after each failed attempt. Defaults to `2`. */
        backoffFactor: z.number().positive().optional(),
        /**
         * Random jitter applied to each retry delay as a fraction of the computed delay.
         * `0.2` means ±20 %. Defaults to `0.2`.
         */
        jitterRatio: z.number().min(0).max(1).optional(),
        /**
         * Predicate that decides whether a given error is retryable.
         * Return `true` to retry, `false` to re-throw immediately.
         * Defaults to always retrying.
         */
        retryableErrors: z
          .custom<
            (
              error: unknown,
              context: { cycle: number; attempt: number; maxAttempts: number }
            ) => boolean
          >(
            (val) => val === undefined || typeof val === "function",
            "retries.retryableErrors must be a function"
          )
          .optional(),
      })
      .optional(),
    metrics: z
      .object({
        /** Port on which to expose a Prometheus `/metrics` endpoint. Omit to disable. */
        prometheusPort: z.number().int().positive().optional(),
        /** Emit structured log lines for every cycle. Defaults to `true`. */
        enableLogging: z.boolean().default(true),
      })
      .default({ enableLogging: true }),
    systemd: z
      .object({
        /** Seconds systemd should wait before restarting the service after a crash. */
        restartSec: z.number().int().positive().optional(),
        /** `nice` priority for the service process (`-20` highest, `19` lowest). */
        nice: z.number().int().optional(),
        /** Unix user the service runs as. Defaults to the invoking user. */
        user: z.string().optional(),
        /** Override the systemd `WorkingDirectory`. Defaults to the project directory. */
        workingDir: z.string().optional(),
        /** Additional environment variables injected into the service unit. */
        environment: z.record(z.string().optional()).optional(),
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
        code: z.ZodIssueCode.custom,
        path: ["retries", "maxDelayMs"],
        message: `retries.maxDelayMs (${config.retries.maxDelayMs}) must be >= retries.initialDelayMs (${config.retries.initialDelayMs}).`,
      });
    }
  });

/**
 * Full configuration object for a Goddard agent loop.
 *
 * Pass a value of this type to `createLoop` to start an autonomous agent
 * cycle. Use {@link defineConfig} to author the config with full type-checking
 * and IDE completions.
 *
 * @example
 * ```ts
 * import { Models, defineConfig } from "@goddard-ai/config";
 *
 * export default defineConfig({
 *   agent: {
 *     model: Models.Anthropic.ClaudeSonnet45,
 *     projectDir: "./",
 *     thinkingLevel: "low",
 *   },
 *   strategy: {
 *     nextPrompt: ({ cycleNumber, lastSummary }) =>
 *       `Cycle ${cycleNumber}. Last: ${lastSummary ?? "none"}. Continue.`,
 *   },
 *   rateLimits: {
 *     cycleDelay: "30m",
 *     maxTokensPerCycle: 128_000,
 *     maxOpsPerMinute: 120,
 *   },
 *   metrics: { enableLogging: true },
 * });
 * ```
 */
export type GoddardLoopConfig = z.infer<typeof configSchema>;

// ---------------------------------------------------------------------------
// defineConfig
// ---------------------------------------------------------------------------

/**
 * Identity helper that types your config object as {@link GoddardLoopConfig}.
 *
 * Wrapping your default export with `defineConfig` gives you full type
 * checking and IDE autocomplete without any runtime overhead — the function
 * simply returns its argument unchanged.
 *
 * @example
 * ```ts
 * // .goddard/config.ts
 * import { Models, defineConfig } from "@goddard-ai/config";
 *
 * export default defineConfig({
 *   agent: { model: Models.Anthropic.ClaudeSonnet45, projectDir: "./" },
 *   strategy: { nextPrompt: () => "Keep improving the codebase." },
 *   rateLimits: { cycleDelay: "30m", maxTokensPerCycle: 128_000, maxOpsPerMinute: 120 },
 * });
 * ```
 */
export function defineConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config;
}
