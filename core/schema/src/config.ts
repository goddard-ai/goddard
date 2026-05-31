import { knownAcpAdapterIds, type AcpAdapterId } from "acp-client"
import * as acp from "acp-client/protocol"
import { z } from "zod"

import { AgentDistribution } from "./agent-distribution.ts"
import { DaemonSessionMetadata } from "./daemon/store.ts"

export const AgentSetting = z.union([
  z.string().min(1) as z.ZodType<AcpAdapterId>,
  AgentDistribution,
])
export type AgentSetting = z.infer<typeof AgentSetting>

/** Schema for persisted agent runtime defaults loaded from JSON. */
export const AgentsConfig = z
  .strictObject({
    default: AgentSetting.optional().describe(
      "Global fallback agent to use when no narrower session or feature config selects an agent.",
    ),
  })
  .describe("Persisted agent runtime defaults loaded from JSON.")

export type AgentsConfig = z.infer<typeof AgentsConfig>

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

/** Schema for persisted daemon connection defaults loaded from JSON. */
export const DaemonConfig = z
  .strictObject({
    port: z
      .number()
      .int()
      .min(1)
      .max(65535)
      .optional()
      .describe(
        "TCP port used by the local daemon control server. Only supported in the global Goddard config.",
      ),
  })
  .describe("Persisted daemon connection defaults loaded from JSON.")

export type DaemonConfig = z.infer<typeof DaemonConfig>

/** Explicit allow/deny policy value for sensitive daemon operations. */
export const SecurityPolicyDecision = z.enum(["allow", "deny"])

export type SecurityPolicyDecision = z.infer<typeof SecurityPolicyDecision>

/** Schema for pull-request operations gated by daemon session tokens. */
export const PullRequestSecurityConfig = z
  .strictObject({
    submit: SecurityPolicyDecision.optional().describe(
      "Whether daemon session tokens may create pull requests.",
    ),
    reply: SecurityPolicyDecision.optional().describe(
      "Whether daemon session tokens may reply to allowed pull requests.",
    ),
  })
  .describe("Pull-request permissions enforced at the daemon session-token boundary.")

export type PullRequestSecurityConfig = z.infer<typeof PullRequestSecurityConfig>

/** Schema for persisted daemon trust and permissions policy loaded from JSON. */
export const SecurityConfig = z
  .strictObject({
    pullRequests: PullRequestSecurityConfig.optional().describe(
      "Policy for pull-request operations performed with daemon session tokens.",
    ),
  })
  .describe("Persisted daemon trust and permissions policy loaded from JSON.")

export type SecurityConfig = z.infer<typeof SecurityConfig>

/** Schema that extracts only the daemon config slice from a root config document. */
export const RootDaemonConfig = z
  .object({
    daemon: DaemonConfig.optional(),
  })
  .passthrough()
  .describe("Root config slice used when only daemon connection defaults need validation.")

/** Reads the daemon config slice without validating unrelated root-config fields. */
export function readDaemonConfigFromRootConfig(value: unknown) {
  return RootDaemonConfig.parse(value).daemon
}

export const ResolvedSessionParams = InlineSessionParams.extend({
  agent: AgentSetting,
  mcpServers: z.array(McpServer),
  cwd: z.string(),
})

export type ResolvedSessionParams = z.infer<typeof ResolvedSessionParams>

export function registerConfigSchemas(acpRegistry: z.core.$ZodRegistry) {
  // Types inherited from ACP schema: https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/schema/schema.json
  acpRegistry.add(McpServer)

  z.globalRegistry.add(AgentSetting, { id: "AgentSetting", examples: [...knownAcpAdapterIds] })
  z.globalRegistry.add(AgentsConfig, { id: "AgentsConfig" })
  z.globalRegistry.add(McpServer, { id: "McpServer" })
  z.globalRegistry.add(DaemonConfig, { id: "DaemonConfig" })
  z.globalRegistry.add(SecurityConfig, { id: "SecurityConfig" })
  z.globalRegistry.add(PullRequestSecurityConfig, { id: "PullRequestSecurityConfig" })
}
