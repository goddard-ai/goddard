import { z } from "zod"

/** Supported reasoning depth values for agent-backed workflows. */
export const ThinkingLevel = z.enum(["off", "minimal", "low", "medium", "high", "xhigh"])

/**
 * Configuration passed through to pi-coding-agent when creating a session.
 * Supports extra provider-specific fields via the index signature.
 */
export const PiAgentConfig = z
  .object({
    model: z.string().describe('pi-coding-agent model string, e.g. "anthropic/claude-opus-4-5"'),
    projectDir: z.string().describe("Absolute path to the project the agent should operate in"),
    thinkingLevel: ThinkingLevel.optional().describe(
      'Thinking / reasoning depth. Defaults to "medium".',
    ),
    agentDir: z
      .string()
      .optional()
      .describe("Override path to the pi agent directory (~/.pi/agent by default)"),
    systemPrompt: z
      .string()
      .optional()
      .describe(
        "System prompt to inject into the agent session. Defaults to LOOP_SYSTEM_PROMPT when running via `goddard loop`. Pass SPEC_SYSTEM_PROMPT when running via `goddard spec`.",
      ),
  })
  .catchall(z.unknown())

export type ThinkingLevel = z.infer<typeof ThinkingLevel>

export type PiAgentConfig = z.infer<typeof PiAgentConfig>
