/** Shared ACP adapter catalog merge helpers for the adapter feature. */
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"

import { AdapterCatalogEntry, type AdapterCatalogEntryType } from "./schema.ts"

/** Sorts adapter catalog entries into a stable user-facing order. */
export function sortAdapterCatalogEntries(entries: AdapterCatalogEntryType[]) {
  return [...entries].sort((left, right) => {
    const nameCompare = left.name.localeCompare(right.name, undefined, {
      sensitivity: "base",
    })
    return nameCompare !== 0 ? nameCompare : left.id.localeCompare(right.id)
  })
}

/** Converts config-declared registry overrides into the shared adapter catalog shape. */
export function createConfigAdapterCatalogEntries(
  registry: Record<string, AgentDistribution> | undefined,
) {
  if (!registry) {
    return []
  }

  return sortAdapterCatalogEntries(
    Object.entries(registry).map(([id, agent]) =>
      AdapterCatalogEntry.parse({
        ...agent,
        id,
        unofficial: id.endsWith("-acp"),
        source: "config",
      }),
    ),
  )
}

/** Applies config-declared registry overrides on top of the upstream adapter catalog. */
export function mergeAdapterCatalogEntries(
  registryEntries: AdapterCatalogEntryType[],
  configEntries: AdapterCatalogEntryType[],
) {
  const mergedById = new Map(registryEntries.map((entry) => [entry.id, entry] as const))

  for (const entry of configEntries) {
    const existing = mergedById.get(entry.id)
    mergedById.set(
      entry.id,
      existing
        ? {
            ...existing,
            ...entry,
            icon: entry.icon ?? existing.icon,
            website: entry.website ?? existing.website,
          }
        : entry,
    )
  }

  return sortAdapterCatalogEntries([...mergedById.values()])
}
