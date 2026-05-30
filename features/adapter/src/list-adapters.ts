import { resolveDefaultAgent } from "@goddard-ai/config"
import type { UserConfig } from "@goddard-ai/schema/config"

import { createConfigAdapterCatalogEntries, mergeAdapterCatalogEntries } from "./catalog.ts"
import {
  AdapterCatalogEntry,
  type ListAdaptersRequestType,
  type ListAdaptersResponse,
} from "./schema.ts"

type AdapterRegistrySnapshot = Omit<ListAdaptersResponse, "adapters" | "defaultAdapterId"> & {
  adapters: readonly unknown[]
}

export type AdapterRegistryService = {
  listAdapters: () => Promise<AdapterRegistrySnapshot>
}

export type AdapterConfigManager = {
  getRootConfig: (cwd: string) => Promise<{ config: UserConfig }>
}

export type ListAdaptersContext = {
  registryService: AdapterRegistryService
  configProvider: AdapterConfigManager
}

/** Lists adapters from registry and config substrate using adapter feature merge semantics. */
export async function listAdapters(
  { registryService, configProvider }: ListAdaptersContext,
  { cwd }: ListAdaptersRequestType,
) {
  const [registrySnapshot, resolvedConfig] = await Promise.all([
    registryService.listAdapters(),
    cwd ? configProvider.getRootConfig(cwd).then((snapshot) => snapshot.config) : undefined,
  ])
  const registryAdapters = registrySnapshot.adapters.map((adapter) =>
    AdapterCatalogEntry.parse(adapter),
  )
  const mergedAdapters = mergeAdapterCatalogEntries(
    registryAdapters,
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
