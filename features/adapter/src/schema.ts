import { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { z } from "zod"

/** Request payload used to list adapters available to one project or global session launch flow. */
export const ListAdaptersRequest = z.strictObject({
  cwd: z.string().optional(),
})

export type ListAdaptersRequest = z.infer<typeof ListAdaptersRequest>
export type ListAdaptersRequestType = ListAdaptersRequest

/** One adapter entry surfaced to SDK and app consumers for launch selection and install flows. */
export const AdapterCatalogEntry = AgentDistribution.extend({
  website: z.url().optional().describe("Optional product or documentation URL for the adapter."),
  icon: z
    .string()
    .optional()
    .describe("Optional icon data URL or other stable icon reference for the adapter."),
  unofficial: z
    .boolean()
    .describe("Whether the adapter id indicates an unofficial ACP integration."),
  source: z
    .enum(["config", "registry"])
    .describe("Where the adapter entry originated before catalog merging."),
})

export type AdapterCatalogEntry = z.infer<typeof AdapterCatalogEntry>
export type AdapterCatalogEntryType = AdapterCatalogEntry

/** Response payload returned after reading the effective adapter catalog for one launch context. */
export type ListAdaptersResponse = {
  adapters: AdapterCatalogEntry[]
  defaultAdapterId: string | null
  registrySource: "cache" | "fallback"
  lastSuccessfulSyncAt: string | null
  stale: boolean
  lastError: string | null
}
