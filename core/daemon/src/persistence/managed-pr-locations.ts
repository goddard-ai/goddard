import { mkdir, readFile, writeFile } from "node:fs/promises"
import { dirname } from "node:path"
import { getManagedPrLocationsPath } from "@goddard-ai/paths/node"

/** Durable local checkout metadata for one managed pull request. */
export type ManagedPrLocationRecord = {
  owner: string
  repo: string
  prNumber: number
  cwd: string
  updatedAt: string
}

/** JSON file shape used to persist managed pull-request checkout lookups. */
type ManagedPrLocationsFile = {
  locations: Record<string, ManagedPrLocationRecord>
}

/** Local storage for mapping managed PRs back to their checkout directories. */
export namespace ManagedPrLocationStorage {
  export async function upsert(
    record: Omit<ManagedPrLocationRecord, "updatedAt">,
  ): Promise<ManagedPrLocationRecord> {
    const data = await readLocationsFile()
    const key = getLocationKey(record.owner, record.repo, record.prNumber)
    data.locations[key] = {
      ...record,
      updatedAt: new Date().toISOString(),
    }
    await writeLocationsFile(data)
    return data.locations[key]!
  }

  export async function get(
    owner: string,
    repo: string,
    prNumber: number,
  ): Promise<ManagedPrLocationRecord | null> {
    const data = await readLocationsFile()
    return data.locations[getLocationKey(owner, repo, prNumber)] ?? null
  }
}

/** Builds the stable storage key for one managed pull-request checkout entry. */
function getLocationKey(owner: string, repo: string, prNumber: number): string {
  return `${owner}/${repo}#${prNumber}`
}

/** Reads the managed pull-request location index, defaulting to an empty store. */
async function readLocationsFile(): Promise<ManagedPrLocationsFile> {
  try {
    const raw = await readFile(getManagedPrLocationsPath(), "utf-8")
    const parsed = JSON.parse(raw) as Partial<ManagedPrLocationsFile>
    return {
      locations: parsed.locations ?? {},
    }
  } catch {
    return { locations: {} }
  }
}

/** Persists the managed pull-request location index back to local storage. */
async function writeLocationsFile(data: ManagedPrLocationsFile): Promise<void> {
  const path = getManagedPrLocationsPath()
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, `${JSON.stringify(data, null, 2)}\n`, "utf-8")
}
