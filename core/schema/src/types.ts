import { z } from "zod"

export const ThinkingLevelSchema = z.enum(["off", "minimal", "low", "medium", "high", "xhigh"])
export type ThinkingLevel = z.infer<typeof ThinkingLevelSchema>

/**
 * Configuration passed through to pi-coding-agent when creating a session.
 * Supports extra provider-specific fields via the index signature.
 */
export const PiAgentConfigSchema = z
  .object({
    model: z.string().describe('pi-coding-agent model string, e.g. "anthropic/claude-opus-4-5"'),
    projectDir: z.string().describe("Absolute path to the project the agent should operate in"),
    thinkingLevel: ThinkingLevelSchema.optional().describe(
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
export type PiAgentConfig = z.infer<typeof PiAgentConfigSchema>

export const RepoRefSchema = z.object({
  owner: z.string(),
  repo: z.string(),
})
export type RepoRef = z.infer<typeof RepoRefSchema>

export const DeviceFlowStartSchema = z.object({
  githubUsername: z.string().optional(),
})
export type DeviceFlowStart = z.infer<typeof DeviceFlowStartSchema>

export const DeviceFlowSessionSchema = z.object({
  deviceCode: z.string(),
  userCode: z.string(),
  verificationUri: z.string(),
  expiresIn: z.number(),
  interval: z.number(),
})
export type DeviceFlowSession = z.infer<typeof DeviceFlowSessionSchema>

export const DeviceFlowCompleteSchema = z.object({
  deviceCode: z.string(),
  githubUsername: z.string(),
})
export type DeviceFlowComplete = z.infer<typeof DeviceFlowCompleteSchema>

export const AuthSessionSchema = z.object({
  token: z.string(),
  githubUsername: z.string(),
  githubUserId: z.number(),
})
export type AuthSession = z.infer<typeof AuthSessionSchema>

export const CreatePrInputSchema = RepoRefSchema.extend({
  title: z.string(),
  body: z.string().optional(),
  head: z.string(),
  base: z.string(),
})
export type CreatePrInput = z.infer<typeof CreatePrInputSchema>

export const ReplyPrInputSchema = RepoRefSchema.extend({
  prNumber: z.number(),
  body: z.string(),
})
export type ReplyPrInput = z.infer<typeof ReplyPrInputSchema>

export const PullRequestRecordSchema = z.object({
  id: z.number(),
  number: z.number(),
  owner: z.string(),
  repo: z.string(),
  title: z.string(),
  body: z.string(),
  head: z.string(),
  base: z.string(),
  url: z.string(),
  createdBy: z.string(),
  createdAt: z.string(),
})
export type PullRequestRecord = z.infer<typeof PullRequestRecordSchema>

export const RepoEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("comment"),
    owner: z.string(),
    repo: z.string(),
    prNumber: z.number(),
    author: z.string(),
    body: z.string(),
    reactionAdded: z.literal("eyes"),
    createdAt: z.string(),
  }),
  z.object({
    type: z.literal("review"),
    owner: z.string(),
    repo: z.string(),
    prNumber: z.number(),
    author: z.string(),
    state: z.enum(["approved", "changes_requested", "commented"]),
    body: z.string(),
    reactionAdded: z.literal("eyes"),
    createdAt: z.string(),
  }),
  z.object({
    type: z.literal("pr.created"),
    owner: z.string(),
    repo: z.string(),
    prNumber: z.number(),
    title: z.string(),
    author: z.string(),
    createdAt: z.string(),
  }),
])
export type RepoEvent = z.infer<typeof RepoEventSchema>

export const StreamMessageSchema = z.object({
  event: RepoEventSchema,
})
export type StreamMessage = z.infer<typeof StreamMessageSchema>

export const GitHubWebhookInputSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("issue_comment"),
    owner: z.string(),
    repo: z.string(),
    prNumber: z.number(),
    author: z.string(),
    body: z.string(),
  }),
  z.object({
    type: z.literal("pull_request_review"),
    owner: z.string(),
    repo: z.string(),
    prNumber: z.number(),
    author: z.string(),
    state: z.enum(["approved", "changes_requested", "commented"]),
    body: z.string(),
  }),
])
export type GitHubWebhookInput = z.infer<typeof GitHubWebhookInputSchema>
