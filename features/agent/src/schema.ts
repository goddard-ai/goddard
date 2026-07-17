import { AdapterCatalogEntry as AcpAdapterCatalogEntry } from "acp-client"
import { z } from "zod"

/** Request payload used to list agents available to one launch flow. */
export const ListAgentsRequest = z.strictObject({
  cwd: z.string().optional(),
  includeUninstalled: z.boolean().optional(),
})

export type ListAgentsRequest = z.infer<typeof ListAgentsRequest>
export type ListAgentsRequestType = ListAgentsRequest

/** Sanitized installed-agent metadata surfaced with managed install status. */
export const ManagedAgentInstallAgent = z.strictObject({
  agentId: z.string(),
  version: z.string(),
  method: z.enum(["binary", "npx", "uvx"]),
  installedAt: z.string(),
  updatedAt: z.string(),
})

export type ManagedAgentInstallAgent = z.infer<typeof ManagedAgentInstallAgent>

export const ManagedAgentInstallState = z.discriminatedUnion("status", [
  z.strictObject({
    status: z.literal("missing"),
  }),
  z.strictObject({
    status: z.literal("installed"),
    agent: ManagedAgentInstallAgent,
  }),
  z.strictObject({
    status: z.literal("failed"),
    lastError: z.string(),
    checkedAt: z.string(),
    agent: ManagedAgentInstallAgent.optional(),
  }),
])

export type ManagedAgentInstallState = z.infer<typeof ManagedAgentInstallState>

export const ManagedAgentInstall = z.strictObject({
  managed: z.literal(true),
  install: z.literal("beforeUse").optional(),
  update: z.literal("daily").optional(),
  state: ManagedAgentInstallState,
})

export type ManagedAgentInstall = z.infer<typeof ManagedAgentInstall>

export const AgentCatalogEntry = AcpAdapterCatalogEntry.extend({
  managedInstall: ManagedAgentInstall.optional(),
})

export type AgentCatalogEntry = z.infer<typeof AgentCatalogEntry>

/** Local launch visibility and installability state for one agent catalog entry. */
export type AgentInstallationState = {
  agentId: string
  installed: boolean
  installable: boolean
  method: "binary" | "config" | "npx" | "unsupported" | "uvx"
}

/** Response payload returned after reading the effective agent catalog. */
export type ListAgentsResponse = {
  agents: AgentCatalogEntry[]
  installations: AgentInstallationState[]
  defaultAgentId: string | null
  registrySource: "cache" | "fallback"
  lastSuccessfulSyncAt: string | null
  stale: boolean
  lastError: string | null
}

/** Request payload used to install one registry agent into the local launch catalog. */
export const InstallAgentRequest = z.strictObject({
  agentId: z.string().min(1),
})

export type InstallAgentRequest = z.infer<typeof InstallAgentRequest>

/** Response payload returned after installing one agent. */
export type InstallAgentResponse = {
  agent: AgentCatalogEntry
  installation: AgentInstallationState
}

/** Request payload used to remove one agent from the local launch catalog. */
export const UninstallAgentRequest = z.strictObject({
  agentId: z.string().min(1),
})

export type UninstallAgentRequest = z.infer<typeof UninstallAgentRequest>

/** Response payload returned after uninstalling one agent. */
export type UninstallAgentResponse = {
  agentId: string
}
