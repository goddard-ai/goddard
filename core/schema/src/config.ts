import * as acp from "@agentclientprotocol/sdk"
import { textModelConfigSchema, type ModelConfig } from "ai-sdk-json-schema"
import { z } from "zod"

import { ACPAdapterNames, type ACPAdapterName } from "./acp-adapters.ts"
import { AgentDistribution } from "./agent-distribution.ts"
import { DaemonSessionMetadata } from "./daemon/store.ts"

export const AgentSetting = z.union([
  z.string().min(1) as z.ZodType<ACPAdapterName>,
  AgentDistribution,
])
export type AgentSetting = z.infer<typeof AgentSetting>

export const McpServer = z.unknown() as z.ZodType<acp.McpServer>
export type McpServer = z.infer<typeof McpServer>

export const StaticSessionParams = z
  .strictObject({
    agent: AgentSetting.optional().describe(
      "Agent to run. Use an installed adapter id or an inline agent distribution manifest.",
    ),
    mcpServers: z
      .array(McpServer)
      .optional()
      .describe("Additional MCP server definitions to attach to the session."),
    env: z
      .record(z.string(), z.string())
      .optional()
      .describe("Environment variables to inject into the session process."),
    model: z
      .string()
      .optional()
      .describe("Model identifier to set for the session upon initialization."),
  })
  .describe("Persisted session defaults that are safe to store in shared JSON config.")

export type StaticSessionParams = z.infer<typeof StaticSessionParams>

export const InlineSessionParams = StaticSessionParams.extend({
  cwd: z
    .string()
    .min(1)
    .optional()
    .describe("Working directory to use when launching the session."),
  systemPrompt: z
    .string()
    .min(1)
    .optional()
    .describe("System prompt to prepend to the session before user messages are sent."),
  repository: z
    .string()
    .min(1)
    .optional()
    .describe("Repository slug or identifier associated with the session's work."),
  prNumber: z
    .number()
    .int()
    .optional()
    .describe("Pull request number associated with the session context."),
  metadata: DaemonSessionMetadata.optional().describe(
    "Additional daemon session metadata shared with the runtime.",
  ),
}).describe("Runtime session settings after ephemeral invocation overrides are applied.")

export type InlineSessionParams = z.infer<typeof InlineSessionParams>

export const LoopRateLimits = z
  .strictObject({
    cycleDelay: z
      .string()
      .min(1)
      .optional()
      .describe("Delay between loop cycles, expressed as a duration string."),
    maxOpsPerMinute: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of tool or model operations the loop may start per minute."),
    maxCyclesBeforePause: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of completed loop cycles before the loop pauses itself."),
  })
  .describe("Rate limits that control how aggressively a loop may run.")

export type LoopRateLimits = z.infer<typeof LoopRateLimits>

export const LoopRetryPolicy = z
  .strictObject({
    maxAttempts: z
      .number()
      .int()
      .positive()
      .optional()
      .describe("Maximum number of retry attempts after a loop operation fails."),
    initialDelayMs: z
      .number()
      .int()
      .nonnegative()
      .optional()
      .describe("Initial retry delay in milliseconds before the first retry."),
    maxDelayMs: z
      .number()
      .int()
      .nonnegative()
      .optional()
      .describe("Maximum retry delay in milliseconds after exponential backoff is applied."),
    backoffFactor: z
      .number()
      .positive()
      .optional()
      .describe("Multiplier applied to the retry delay after each failed attempt."),
    jitterRatio: z
      .number()
      .nonnegative()
      .optional()
      .describe("Random jitter ratio applied to retry delays to avoid synchronized retries."),
  })
  .describe("Retry policy used when loop operations fail.")

export type LoopRetryPolicy = z.infer<typeof LoopRetryPolicy>

/** Schema for persisted action defaults loaded from JSON. */
export const ActionConfig = z.strictObject({
  session: StaticSessionParams.optional().describe(
    "Default session settings applied to all agent actions.",
  ),
})

export type ActionConfig = z.infer<typeof ActionConfig>

/** Schema for persisted loop defaults loaded from JSON. */
export const LoopConfig = z
  .strictObject({
    session: StaticSessionParams.optional().describe(
      "Default session settings applied to loop-backed agent runs.",
    ),
    rateLimits: LoopRateLimits.optional().describe(
      "Loop pacing limits that bound runtime throughput.",
    ),
    retries: LoopRetryPolicy.optional().describe(
      "Retry settings used when a loop cycle or operation fails.",
    ),
  })
  .describe("Persisted loop defaults loaded from JSON.")

export type LoopConfig = z.infer<typeof LoopConfig>

/** Schema for one custom worktree plugin loaded from a filesystem path. */
export const WorktreePluginPathReference = z
  .strictObject({
    type: z.literal("path"),
    path: z
      .string()
      .min(1)
      .describe(
        "Absolute plugin module path or a path resolved relative to the Goddard global directory.",
      ),
    export: z
      .string()
      .min(1)
      .optional()
      .describe("Optional module export name to load. Defaults to `default`."),
  })
  .describe("Reference to a custom worktree plugin loaded from a module path.")

export type WorktreePluginPathReference = z.infer<typeof WorktreePluginPathReference>

/** Schema for one custom worktree plugin loaded from a globally installed package. */
export const WorktreePluginPackageReference = z
  .strictObject({
    type: z.literal("package"),
    package: z
      .string()
      .min(1)
      .describe("Package name for a globally installed worktree plugin module."),
    export: z
      .string()
      .min(1)
      .optional()
      .describe("Optional module export name to load. Defaults to `default`."),
  })
  .describe("Reference to a custom worktree plugin loaded from a globally installed package.")

export type WorktreePluginPackageReference = z.infer<typeof WorktreePluginPackageReference>

/** Schema for one custom worktree plugin reference declared in root config. */
export const WorktreePluginReference = z
  .discriminatedUnion("type", [WorktreePluginPathReference, WorktreePluginPackageReference])
  .describe("Custom worktree plugin reference loaded by the daemon from global config.")

export type WorktreePluginReference = z.infer<typeof WorktreePluginReference>

/** Schema for supported package managers used by daemon-managed worktree bootstrap. */
export const WorktreeBootstrapPackageManager = z
  .enum(["bun", "pnpm", "npm", "yarn"])
  .describe("Package manager command used to prepare fresh daemon-managed worktrees.")

export type WorktreeBootstrapPackageManager = z.infer<typeof WorktreeBootstrapPackageManager>

/** Schema for daemon-managed bootstrap defaults applied to fresh worktrees. */
export const WorktreeBootstrapConfig = z
  .strictObject({
    enabled: z
      .boolean()
      .optional()
      .describe("Whether daemon-managed worktree seeding and bootstrap are enabled."),
    packageManager: WorktreeBootstrapPackageManager.optional().describe(
      "Package manager command to run when bootstrapping a fresh daemon-managed worktree.",
    ),
    installArgs: z
      .array(z.string().min(1))
      .optional()
      .describe("Additional arguments appended to the selected package-manager install command."),
    seedEnabled: z
      .boolean()
      .optional()
      .describe("Whether selected untracked artifacts should be copied into fresh worktrees."),
    seedNames: z
      .array(z.string().min(1))
      .optional()
      .describe("Recursive basename allowlist used when selecting untracked seed candidates."),
    seedPaths: z
      .array(z.string().min(1))
      .optional()
      .describe("Exact repository-relative paths added to the untracked seed candidate set."),
  })
  .describe("Daemon-managed preparation settings applied to fresh worktrees.")

export type WorktreeBootstrapConfig = z.infer<typeof WorktreeBootstrapConfig>

/** Schema for persisted daemon worktree defaults loaded from JSON. */
export const WorktreesConfig = z
  .strictObject({
    defaultFolder: z
      .string()
      .min(1)
      .optional()
      .describe("Default repository-local folder name used for daemon-managed worktrees."),
    bootstrap: WorktreeBootstrapConfig.optional().describe(
      "Daemon-managed preparation defaults applied to fresh worktrees.",
    ),
    plugins: z
      .array(WorktreePluginReference)
      .optional()
      .describe("Custom worktree plugins loaded from the global Goddard config only."),
  })
  .describe("Persisted worktree defaults loaded from JSON.")

export type WorktreesConfig = z.infer<typeof WorktreesConfig>

/** Schema for persisted session-title generation defaults loaded from JSON. */
export const SessionTitlesConfig = z
  .strictObject({
    generator: textModelConfigSchema
      .optional()
      .describe("Text model selection used for background session title generation."),
  })
  .describe("Persisted session title-generation defaults loaded from JSON.")

export type SessionTitlesConfig = z.infer<typeof SessionTitlesConfig> & {
  generator?: ModelConfig
}

/** Schema for the shared root config document. */
export const UserConfig = z
  .strictObject({
    worktrees: WorktreesConfig.optional().describe(
      "Default settings for daemon-managed worktrees.",
    ),
    sessionTitles: SessionTitlesConfig.optional().describe(
      "Default settings for background session title generation.",
    ),
    session: StaticSessionParams.optional().describe(
      "Default session settings applied to all sessions.",
    ),
    actions: ActionConfig.optional().describe("Default settings for agent actions."),
    loops: LoopConfig.optional().describe("Default settings for long-running agent loops."),
    registry: z
      .record(z.string(), AgentDistribution)
      .optional()
      .describe("Custom registry of ACP agent distributions."),
  })
  .describe("Shared root config document loaded from local and global JSON files.")

export type UserConfig = z.infer<typeof UserConfig>

/** Schema for resolved loop rate limits with all required fields present. */
export const ResolvedLoopRateLimits = z.object({
  cycleDelay: z.string().min(1),
  maxOpsPerMinute: z.number().int().positive(),
  maxCyclesBeforePause: z.number().int().positive(),
})

export type ResolvedLoopRateLimits = z.infer<typeof ResolvedLoopRateLimits>

/** Schema for resolved loop retry settings with all required fields present. */
export const ResolvedLoopRetries = z.object({
  maxAttempts: z.number().int().positive(),
  initialDelayMs: z.number().int().nonnegative(),
  maxDelayMs: z.number().int().nonnegative(),
  backoffFactor: z.number().positive(),
  jitterRatio: z.number().nonnegative(),
})

export type ResolvedLoopRetries = z.infer<typeof ResolvedLoopRetries>

export const ResolvedSessionParams = InlineSessionParams.extend({
  agent: AgentSetting,
  mcpServers: z.array(McpServer),
  cwd: z.string(),
})

export type ResolvedSessionParams = z.infer<typeof ResolvedSessionParams>

/** Schema for a fully resolved loop config document. */
export const ResolvedLoopConfig = z.object({
  session: ResolvedSessionParams,
  rateLimits: ResolvedLoopRateLimits,
  retries: ResolvedLoopRetries,
})

export type ResolvedLoopConfig = z.infer<typeof ResolvedLoopConfig>

export function registerConfigSchemas(acpRegistry: z.core.$ZodRegistry) {
  // Types inherited from ACP schema: https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/schema/schema.json
  acpRegistry.add(McpServer)

  z.globalRegistry.add(AgentSetting, {
    id: "AgentSetting",
    examples: [...ACPAdapterNames],
  })
  z.globalRegistry.add(McpServer, { id: "McpServer" })
  z.globalRegistry.add(ActionConfig, { id: "ActionConfig" })
  z.globalRegistry.add(LoopConfig, { id: "LoopConfig" })
  z.globalRegistry.add(WorktreePluginPathReference, {
    id: "WorktreePluginPathReference",
  })
  z.globalRegistry.add(WorktreePluginPackageReference, {
    id: "WorktreePluginPackageReference",
  })
  z.globalRegistry.add(WorktreePluginReference, {
    id: "WorktreePluginReference",
  })
  z.globalRegistry.add(WorktreeBootstrapPackageManager, {
    id: "WorktreeBootstrapPackageManager",
  })
  z.globalRegistry.add(WorktreeBootstrapConfig, {
    id: "WorktreeBootstrapConfig",
  })
  z.globalRegistry.add(WorktreesConfig, { id: "WorktreesConfig" })
  z.globalRegistry.add(SessionTitlesConfig, { id: "SessionTitlesConfig" })
  z.globalRegistry.add(UserConfig, { id: "RootConfig" })
}
