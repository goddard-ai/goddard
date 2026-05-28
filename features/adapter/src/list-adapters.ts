import { resolveDefaultAgent } from "@goddard-ai/config"
import type { UserConfig } from "@goddard-ai/schema/config"

import { createConfigAdapterCatalogEntries, mergeAdapterCatalogEntries } from "./catalog.ts"
import type {
  AdapterCatalogEntryType,
  ListAdaptersRequestType,
  ListAdaptersResponse,
} from "./schema.ts"

type AdapterRegistrySnapshot = Omit<ListAdaptersResponse, "adapters" | "defaultAdapterId"> & {
  adapters: AdapterCatalogEntryType[]
}

export type AdapterRegistryService = {
  listAdapters: () => Promise<AdapterRegistrySnapshot>
}

export type AdapterConfigManager = {
  getRootConfig: (cwd: string) => Promise<{ config: UserConfig }>
}

export type ListAdaptersContext = {
  registryService: AdapterRegistryService
  configManager: AdapterConfigManager
}

/** Lists adapters from registry and config substrate using adapter feature merge semantics. */
export async function listAdapters(
  { registryService, configManager }: ListAdaptersContext,
  { cwd }: ListAdaptersRequestType,
) {
  const [registrySnapshot, resolvedConfig] = await Promise.all([
    registryService.listAdapters(),
    cwd ? configManager.getRootConfig(cwd).then((snapshot) => snapshot.config) : undefined,
  ])
  const mergedAdapters = mergeAdapterCatalogEntries(
    registrySnapshot.adapters,
    createConfigAdapterCatalogEntries(resolvedConfig?.registry),
  )
  const defaultAgent = await resolveDefaultAgent(resolvedConfig)

  return {
    ...registrySnapshot,
    adapters: mergedAdapters,
    defaultAdapterId:
      typeof defaultAgent === "string" &&
      mergedAdapters.some((adapter) => adapter.id === defaultAgent)
        ? defaultAgent
        : null,
  }
}
