import { AdapterCatalogEntry as AcpAdapterCatalogEntry } from "acp-client"
import { z } from "zod"

/** Request payload used to list managed agents available to one launch flow. */
export const ListManagedAgentsRequest = z.strictObject({
  cwd: z.string().optional(),
  includeUninstalled: z.boolean().optional(),
})

export type ListManagedAgentsRequest = z.infer<typeof ListManagedAgentsRequest>
export type ListManagedAgentsRequestType = ListManagedAgentsRequest

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

export const ManagedAgentCatalogEntry = AcpAdapterCatalogEntry.extend({
  managedInstall: ManagedAgentInstall.optional(),
})

export type ManagedAgentCatalogEntry = z.infer<typeof ManagedAgentCatalogEntry>

/** Local launch visibility and installability state for one managed-agent catalog entry. */
export type ManagedAgentInstallationState = {
  managedAgentId: string
  installed: boolean
  installable: boolean
  method: "binary" | "config" | "npx" | "unsupported" | "uvx"
}

/** Response payload returned after reading the effective managed-agent catalog. */
export type ListManagedAgentsResponse = {
  managedAgents: ManagedAgentCatalogEntry[]
  installations: ManagedAgentInstallationState[]
  defaultManagedAgentId: string | null
  registrySource: "cache" | "fallback"
  lastSuccessfulSyncAt: string | null
  stale: boolean
  lastError: string | null
}

/** Request payload used to install one registry agent into the local launch catalog. */
export const InstallManagedAgentRequest = z.strictObject({
  managedAgentId: z.string().min(1),
})

export type InstallManagedAgentRequest = z.infer<typeof InstallManagedAgentRequest>

/** Response payload returned after installing one managed agent. */
export type InstallManagedAgentResponse = {
  managedAgent: ManagedAgentCatalogEntry
  installation: ManagedAgentInstallationState
}

/** Request payload used to remove one agent from the local launch catalog. */
export const UninstallManagedAgentRequest = z.strictObject({
  managedAgentId: z.string().min(1),
})

export type UninstallManagedAgentRequest = z.infer<typeof UninstallManagedAgentRequest>

/** Response payload returned after uninstalling one managed agent. */
export type UninstallManagedAgentResponse = {
  managedAgentId: string
}
