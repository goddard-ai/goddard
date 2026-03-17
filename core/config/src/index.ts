import { z } from "zod"

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

export const Models = {
  Anthropic: {
    Claude37Sonnet: "anthropic/claude-3-7-sonnet-20250219",
    ClaudeSonnet45: "anthropic/claude-sonnet-4-5",
    ClaudeSonnet46: "anthropic/claude-sonnet-4-6",
    ClaudeOpus46: "anthropic/claude-opus-4-6",
  },
  OpenAi: {
    O3Mini: "openai/o3-mini",
    O3Pro: "openai/o3-pro",
    Gpt5Codex: "openai/gpt-5-codex",
    Gpt51Codex: "openai/gpt-5.1-codex",
    Gpt52Codex: "openai/gpt-5.2-codex",
    Gpt53Codex: "openai/gpt-5.3-codex",
  },
} as const

type ValueOf<T> = T[keyof T]

export type Model = ValueOf<typeof Models.Anthropic> | ValueOf<typeof Models.OpenAi> | (string & {})

// ---------------------------------------------------------------------------
// Primitive schemas
// ---------------------------------------------------------------------------

const thinkingLevelSchema = z.enum(["off", "minimal", "low", "medium", "high", "xhigh"])

export type ThinkingLevel = z.infer<typeof thinkingLevelSchema>

// ---------------------------------------------------------------------------
// Agent sub-schema
// ---------------------------------------------------------------------------

const agentSchema = z
  .object({
    model: z.string().min(1) as z.ZodType<Model>,
    projectDir: z.string().min(1),
    thinkingLevel: thinkingLevelSchema.optional(),
    agentDir: z.string().optional(),
  })
  .passthrough()

export type PiAgentConfig = z.infer<typeof agentSchema>

// ---------------------------------------------------------------------------
// configSchema (top-level)
// ---------------------------------------------------------------------------

/**
 * File-based loop settings are no longer modeled here.
 * The config file remains a passthrough object so callers can continue using
 * `defineConfig`, while runtime loop settings are sourced elsewhere.
 */
export const configSchema = z.object({}).passthrough()

export type GoddardLoopConfig = z.infer<typeof configSchema>

// ---------------------------------------------------------------------------
// defineConfig
// ---------------------------------------------------------------------------

export function defineConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config
}
