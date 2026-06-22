/** Shared ACP agent catalog merge helpers for the managed-agent feature. */
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"

import { ManagedAgentCatalogEntry } from "./schema.ts"

/** Sorts managed-agent catalog entries into a stable user-facing order. */
export function sortManagedAgentCatalogEntries(entries: ManagedAgentCatalogEntry[]) {
  return [...entries].sort((left, right) => {
    const nameCompare = left.name.localeCompare(right.name, undefined, {
      sensitivity: "base",
    })
    return nameCompare !== 0 ? nameCompare : left.id.localeCompare(right.id)
  })
}

/** Converts config-declared registry overrides into the shared managed-agent catalog shape. */
export function createConfigManagedAgentCatalogEntries(
  registry: Record<string, AgentDistribution> | undefined,
) {
  if (!registry) {
    return []
  }

  return sortManagedAgentCatalogEntries(
    Object.entries(registry).map(([id, agent]) =>
      ManagedAgentCatalogEntry.parse({
        ...agent,
        id,
        unofficial: id.endsWith("-acp"),
        source: "config",
      }),
    ),
  )
}

/** Applies config-declared registry overrides on top of the upstream catalog. */
export function mergeManagedAgentCatalogEntries(
  registryEntries: ManagedAgentCatalogEntry[],
  configEntries: ManagedAgentCatalogEntry[],
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

  return sortManagedAgentCatalogEntries([...mergedById.values()])
}
