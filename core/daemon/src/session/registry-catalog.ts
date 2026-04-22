/** Shared ACP registry catalog parsing helpers used by both runtime and code generation. */
import { access, readdir, readFile } from "node:fs/promises"
import { join } from "node:path"
import {
  AgentDistribution as AgentDistributionSchema,
  type AgentDistribution,
} from "@goddard-ai/schema/agent-distribution"
import {
  AdapterCatalogEntry,
  type AdapterCatalogEntry as AdapterCatalogEntryType,
} from "@goddard-ai/schema/daemon"
import { z } from "zod"

const UpstreamRegistryAgent = AgentDistributionSchema.extend({
  website: z.url().optional(),
  icon: z.string().optional(),
})

type UpstreamRegistryAgent = z.infer<typeof UpstreamRegistryAgent>

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

/** Reads one registry-like directory tree into the shared adapter catalog shape. */
export async function readAdapterCatalogFromRegistryDir(
  rootDir: string,
): Promise<AdapterCatalogEntry[]> {
  const entries = await readdir(rootDir, { withFileTypes: true })
  const adapters = await Promise.all(
    entries
      .filter((entry) => entry.isDirectory() && entry.name.startsWith(".") === false)
      .map(async (entry) => readRegistryAgent(join(rootDir, entry.name))),
  )

  return sortAdapterCatalogEntries(
    adapters.filter((entry): entry is AdapterCatalogEntryType => entry !== null),
  )
}

/** Parses one registry adapter directory, including its sibling icon asset when present. */
async function readRegistryAgent(agentDir: string): Promise<AdapterCatalogEntry | null> {
  const manifestPath = join(agentDir, "agent.json")
  if ((await pathExists(manifestPath)) === false) {
    return null
  }

  const parsed = UpstreamRegistryAgent.parse(JSON.parse(await readFile(manifestPath, "utf8")))
  return AdapterCatalogEntry.parse({
    ...parsed,
    icon: (await readSvgIconDataUrl(agentDir)) ?? parsed.icon,
    unofficial: parsed.id.endsWith("-acp"),
    source: "registry",
  })
}

/** Reads one sibling SVG icon and encodes it as a browser-safe data URL. */
async function readSvgIconDataUrl(agentDir: string) {
  const iconPath = join(agentDir, "icon.svg")
  if ((await pathExists(iconPath)) === false) {
    return null
  }

  return toSvgDataUrl(await readFile(iconPath, "utf8"))
}

/** Encodes one SVG payload into a stable data URL for app-side rendering. */
export function toSvgDataUrl(svg: string) {
  return `data:image/svg+xml;base64,${Buffer.from(svg, "utf8").toString("base64")}`
}

/** Returns whether one filesystem path currently exists. */
async function pathExists(path: string) {
  try {
    await access(path)
    return true
  } catch {
    return false
  }
}
