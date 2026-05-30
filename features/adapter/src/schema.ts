import type { AdapterCatalogEntry } from "acp-client"
import { z } from "zod"

/** Request payload used to list adapters available to one project or global session launch flow. */
export const ListAdaptersRequest = z.strictObject({
  cwd: z.string().optional(),
})

export type ListAdaptersRequest = z.infer<typeof ListAdaptersRequest>
export type ListAdaptersRequestType = ListAdaptersRequest

/** One adapter entry surfaced to SDK and app consumers for launch selection and install flows. */
export { AdapterCatalogEntry } from "acp-client"

/** Response payload returned after reading the effective adapter catalog for one launch context. */
export type ListAdaptersResponse = {
  adapters: AdapterCatalogEntry[]
  defaultAdapterId: string | null
  registrySource: "cache" | "fallback"
  lastSuccessfulSyncAt: string | null
  stale: boolean
  lastError: string | null
}
