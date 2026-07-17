/** Shared ACP agent catalog merge helpers for the agent feature. */
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"

import { AgentCatalogEntry } from "./schema.ts"

/** Sorts agent catalog entries into a stable user-facing order. */
export function sortAgentCatalogEntries(entries: AgentCatalogEntry[]) {
  return [...entries].sort((left, right) => {
    const nameCompare = left.name.localeCompare(right.name, undefined, {
      sensitivity: "base",
    })
    return nameCompare !== 0 ? nameCompare : left.id.localeCompare(right.id)
  })
}

/** Converts config-declared registry overrides into the shared agent catalog shape. */
export function createConfigAgentCatalogEntries(
  registry: Record<string, AgentDistribution> | undefined,
) {
  if (!registry) {
    return []
  }

  return sortAgentCatalogEntries(
    Object.entries(registry).map(([id, agent]) =>
      AgentCatalogEntry.parse({
        ...agent,
        id,
        unofficial: id.endsWith("-acp"),
        source: "config",
      }),
    ),
  )
}

/** Applies config-declared registry overrides on top of the upstream catalog. */
export function mergeAgentCatalogEntries(
  registryEntries: AgentCatalogEntry[],
  configEntries: AgentCatalogEntry[],
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

  return sortAgentCatalogEntries([...mergedById.values()])
}
