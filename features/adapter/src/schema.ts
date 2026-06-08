import { AdapterCatalogEntry as AcpAdapterCatalogEntry } from "acp-client"
import { z } from "zod"

/** Request payload used to list adapters available to one project or global session launch flow. */
export const ListAdaptersRequest = z.strictObject({
  cwd: z.string().optional(),
  includeUninstalled: z.boolean().optional(),
})

export type ListAdaptersRequest = z.infer<typeof ListAdaptersRequest>
export type ListAdaptersRequestType = ListAdaptersRequest

/** One adapter entry surfaced to SDK and app consumers for launch selection and install flows. */
export const AdapterManagedInstallAgent = z.strictObject({
  agentId: z.string(),
  version: z.string(),
  method: z.enum(["binary", "npx", "uvx"]),
  installedAt: z.string(),
  updatedAt: z.string(),
})

export type AdapterManagedInstallAgent = z.infer<typeof AdapterManagedInstallAgent>

export const AdapterManagedInstallState = z.discriminatedUnion("status", [
  z.strictObject({
    status: z.literal("missing"),
  }),
  z.strictObject({
    status: z.literal("installed"),
    agent: AdapterManagedInstallAgent,
  }),
  z.strictObject({
    status: z.literal("failed"),
    lastError: z.string(),
    checkedAt: z.string(),
    agent: AdapterManagedInstallAgent.optional(),
  }),
])

export type AdapterManagedInstallState = z.infer<typeof AdapterManagedInstallState>

export const AdapterManagedInstall = z.strictObject({
  managed: z.literal(true),
  install: z.literal("beforeUse").optional(),
  update: z.literal("daily").optional(),
  state: AdapterManagedInstallState,
})

export type AdapterManagedInstall = z.infer<typeof AdapterManagedInstall>

export const AdapterCatalogEntry = AcpAdapterCatalogEntry.extend({
  managedInstall: AdapterManagedInstall.optional(),
})

export type AdapterCatalogEntry = z.infer<typeof AdapterCatalogEntry>

/** Installation state for one ACP adapter catalog entry. */
export type AdapterInstallationState = {
  adapterId: string
  installed: boolean
  installable: boolean
  method: "binary" | "config" | "npx" | "unsupported" | "uvx"
}

/** Response payload returned after reading the effective adapter catalog for one launch context. */
export type ListAdaptersResponse = {
  adapters: AdapterCatalogEntry[]
  installations: AdapterInstallationState[]
  defaultAdapterId: string | null
  registrySource: "cache" | "fallback"
  lastSuccessfulSyncAt: string | null
  stale: boolean
  lastError: string | null
}

/** Request payload used to install one registry adapter into the local launch catalog. */
export const InstallAdapterRequest = z.strictObject({
  adapterId: z.string().min(1),
})

export type InstallAdapterRequest = z.infer<typeof InstallAdapterRequest>

/** Response payload returned after installing one adapter. */
export type InstallAdapterResponse = {
  adapter: AdapterCatalogEntry
  installation: AdapterInstallationState
}

/** Request payload used to remove one adapter from the local launch catalog. */
export const UninstallAdapterRequest = z.strictObject({
  adapterId: z.string().min(1),
})

export type UninstallAdapterRequest = z.infer<typeof UninstallAdapterRequest>

/** Response payload returned after uninstalling one adapter. */
export type UninstallAdapterResponse = {
  adapterId: string
}
