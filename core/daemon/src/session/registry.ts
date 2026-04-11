/** Daemon-owned ACP registry cache and fallback catalog service. */
import { getAcpRegistryCacheDir } from "@goddard-ai/paths/node"
import type { AdapterCatalogEntry } from "@goddard-ai/schema/daemon"
import { access, mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import { readAdapterCatalogFromRegistryDir } from "./registry-catalog.ts"
import { ACPRegistryFallbackCatalog } from "./registry-fallback.ts"

const ACP_REGISTRY_URL = "https://github.com/agentclientprotocol/registry"
const registryStateFileName = ".goddard-registry-state.json"
const registryCloneTtlMs = 60 * 60 * 1000

type RegistryState = {
  lastAttemptedSyncAt: string | null
  lastSuccessfulSyncAt: string | null
  lastError: string | null
}

type RegistryCatalogSnapshot = {
  adapters: AdapterCatalogEntry[]
  registrySource: "cache" | "fallback"
  lastSuccessfulSyncAt: string | null
  stale: boolean
  lastError: string | null
}

/** Optional runtime overrides used by tests and daemon startup. */
type ACPRegistryServiceOptions = {
  cacheDir?: string
  registryUrl?: string
  now?: () => number
  cloneTtlMs?: number
}

/** Public daemon surface for reading adapter catalog state from the registry cache. */
export type ACPRegistryService = {
  listAdapters: () => Promise<RegistryCatalogSnapshot>
  getAdapter: (
    id: string,
  ) => Promise<RegistryCatalogSnapshot & { adapter: AdapterCatalogEntry | null }>
}

/** Creates the ACP registry cache service used by session launch and the adapter listing API. */
export function createACPRegistryService(options: ACPRegistryServiceOptions = {}) {
  const cacheDir = options.cacheDir ?? getAcpRegistryCacheDir()
  const registryUrl = options.registryUrl ?? ACP_REGISTRY_URL
  const now = options.now ?? Date.now
  const cloneTtlMs = options.cloneTtlMs ?? registryCloneTtlMs
  let inFlightSync: Promise<RegistryCatalogSnapshot> | null = null

  async function listAdapters() {
    const snapshotPromise =
      inFlightSync ??
      (inFlightSync = syncAndReadCatalog().finally(() => {
        inFlightSync = null
      }))

    return await snapshotPromise
  }

  async function getAdapter(id: string) {
    const snapshot = await listAdapters()
    return {
      ...snapshot,
      adapter: snapshot.adapters.find((adapter) => adapter.id === id) ?? null,
    }
  }

  async function syncAndReadCatalog(): Promise<RegistryCatalogSnapshot> {
    const state = await readRegistryState(cacheDir)
    const hasClone = await pathExists(join(cacheDir, ".git"))

    if (!hasClone) {
      try {
        await cloneRegistryRepo({
          cacheDir,
          registryUrl,
          attemptedAt: toIsoTimestamp(now()),
        })
        return await readCatalogFromCache(cacheDir, {
          state: await readRegistryState(cacheDir),
          stale: false,
        })
      } catch (error) {
        return buildFallbackSnapshot({
          state: {
            ...state,
            lastAttemptedSyncAt: toIsoTimestamp(now()),
            lastError: error instanceof Error ? error.message : String(error),
          },
        })
      }
    }

    const stale = isRegistryStateStale(state, now(), cloneTtlMs)
    if (!stale) {
      try {
        return await readCatalogFromCache(cacheDir, { state, stale: false })
      } catch (error) {
        return buildFallbackSnapshot({
          state: {
            ...state,
            lastError: error instanceof Error ? error.message : String(error),
          },
        })
      }
    }

    try {
      await refreshRegistryClone({
        cacheDir,
        attemptedAt: toIsoTimestamp(now()),
      })
      return await readCatalogFromCache(cacheDir, {
        state: await readRegistryState(cacheDir),
        stale: false,
      })
    } catch (error) {
      const nextState = {
        ...state,
        lastAttemptedSyncAt: toIsoTimestamp(now()),
        lastError: error instanceof Error ? error.message : String(error),
      } satisfies RegistryState

      if (await pathExists(join(cacheDir, ".git"))) {
        await writeRegistryState(cacheDir, nextState)
        return await readCatalogFromCache(cacheDir, {
          state: nextState,
          stale: true,
        })
      }

      return buildFallbackSnapshot({ state: nextState })
    }
  }

  return {
    listAdapters,
    getAdapter,
  } satisfies ACPRegistryService
}

/** Reads and normalizes the adapter catalog from one cached registry clone. */
async function readCatalogFromCache(
  cacheDir: string,
  input: { state: RegistryState; stale: boolean },
): Promise<RegistryCatalogSnapshot> {
  return {
    adapters: await readAdapterCatalogFromRegistryDir(cacheDir),
    registrySource: "cache" as const,
    lastSuccessfulSyncAt: input.state.lastSuccessfulSyncAt,
    stale: input.stale,
    lastError: input.state.lastError,
  }
}

/** Returns the fallback catalog snapshot when no usable clone is currently available. */
function buildFallbackSnapshot(input: { state: RegistryState }): RegistryCatalogSnapshot {
  return {
    adapters: ACPRegistryFallbackCatalog,
    registrySource: "fallback" as const,
    lastSuccessfulSyncAt: input.state.lastSuccessfulSyncAt,
    stale: true,
    lastError: input.state.lastError,
  }
}

/** Returns whether one cached registry clone should be refreshed before it is read again. */
function isRegistryStateStale(state: RegistryState, currentTimeMs: number, cloneTtlMs: number) {
  if (!state.lastSuccessfulSyncAt) {
    return true
  }

  const lastSuccessfulSyncMs = Date.parse(state.lastSuccessfulSyncAt)
  return (
    Number.isFinite(lastSuccessfulSyncMs) === false ||
    currentTimeMs - lastSuccessfulSyncMs >= cloneTtlMs
  )
}

/** Clones the upstream registry into a staging directory and atomically publishes it into cache. */
async function cloneRegistryRepo(input: {
  cacheDir: string
  registryUrl: string
  attemptedAt: string
}) {
  await mkdir(dirname(input.cacheDir), { recursive: true })
  const stagingParentDir = await mkdtemp(join(dirname(input.cacheDir), "acp-registry-"))
  const stagedCloneDir = join(stagingParentDir, "repo")

  try {
    await runGit(dirname(input.cacheDir), [
      "clone",
      "--depth",
      "1",
      "--single-branch",
      "--branch",
      "main",
      input.registryUrl,
      stagedCloneDir,
    ])
    await writeRegistryState(stagedCloneDir, {
      lastAttemptedSyncAt: input.attemptedAt,
      lastSuccessfulSyncAt: input.attemptedAt,
      lastError: null,
    })
    await rm(input.cacheDir, { recursive: true, force: true })
    await rename(stagedCloneDir, input.cacheDir)
  } finally {
    await rm(stagingParentDir, { recursive: true, force: true })
  }
}

/** Refreshes one cached registry clone in place without replacing the published directory root. */
async function refreshRegistryClone(input: { cacheDir: string; attemptedAt: string }) {
  await runGit(input.cacheDir, ["fetch", "--depth", "1", "origin", "main"])
  await runGit(input.cacheDir, ["reset", "--hard", "FETCH_HEAD"])
  await writeRegistryState(input.cacheDir, {
    lastAttemptedSyncAt: input.attemptedAt,
    lastSuccessfulSyncAt: input.attemptedAt,
    lastError: null,
  })
}

/** Runs one git subprocess and throws when the command does not succeed. */
async function runGit(cwd: string, args: string[]) {
  const result = Bun.spawn(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })

  const [stdout, stderr] = await Promise.all([
    result.stdout ? new Response(result.stdout).text() : "",
    result.stderr ? new Response(result.stderr).text() : "",
    result.exited,
  ])

  if (result.exitCode !== 0) {
    throw new Error(stderr.trim() || stdout.trim() || `git ${args.join(" ")} failed`)
  }
}

/** Reads the daemon-owned registry sync metadata stored alongside the clone root. */
async function readRegistryState(cacheDir: string) {
  try {
    return JSON.parse(
      await readFile(join(cacheDir, registryStateFileName), "utf8"),
    ) as RegistryState
  } catch {
    return {
      lastAttemptedSyncAt: null,
      lastSuccessfulSyncAt: null,
      lastError: null,
    } satisfies RegistryState
  }
}

/** Persists the daemon-owned registry sync metadata inside the cache root. */
async function writeRegistryState(cacheDir: string, state: RegistryState) {
  await writeFile(join(cacheDir, registryStateFileName), JSON.stringify(state, null, 2), "utf8")
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

/** Formats one millisecond timestamp into the persisted sync metadata format. */
function toIsoTimestamp(value: number) {
  return new Date(value).toISOString()
}
