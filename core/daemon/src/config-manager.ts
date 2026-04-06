import chokidar, { type FSWatcher } from "chokidar"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/paths/node"
import type { UserConfig } from "@goddard-ai/schema/config"
import { resolve } from "node:path"
import { createDaemonLogger } from "./logging.ts"
import { readMergedRootConfig } from "./resolvers/config.ts"

const WATCH_STABILITY_MS = 50
const WATCH_POLL_INTERVAL_MS = 10
const RELOAD_DEBOUNCE_MS = 25

/** One validated merged root-config snapshot owned by the daemon config manager. */
export type RootConfigSnapshot = {
  globalRoot: string
  localRoot: string
  config: UserConfig
  version: number
  loadedAt: string
}

/** Daemon-owned contract for serving hot-reloadable persisted root-config snapshots. */
export interface ConfigManager {
  getRootConfig: (cwd?: string) => Promise<RootConfigSnapshot>
  getLastKnownRootConfig: (cwd?: string) => RootConfigSnapshot | null
  ensureWatching: (cwd: string) => Promise<void>
  close: () => Promise<void>
}

type CachedRootConfigEntry = {
  cwd: string
  localRoot: string
  localConfigPath: string
  snapshot: RootConfigSnapshot | null
  watcher: FSWatcher | null
  watcherReady: Promise<void> | null
  reloadTask: Promise<RootConfigSnapshot | null> | null
  debounceHandle: ReturnType<typeof setTimeout> | null
}

/** Creates the daemon-owned config manager for merged persisted root-config snapshots. */
export function createConfigManager() {
  const logger = createDaemonLogger()
  const entries = new Map<string, CachedRootConfigEntry>()
  const globalRoot = getGoddardGlobalDir()
  const globalConfigPath = getGlobalConfigPath()
  let closed = false
  const { watcher: globalWatcher, ready: globalWatcherReady } = createWatcher(
    "global",
    globalRoot,
    globalConfigPath,
    () => {
      for (const entry of entries.values()) {
        scheduleReload(entry, "global")
      }
    },
  )
  logWatcherStarted("global", globalRoot, globalConfigPath)

  function logWatcherStarted(scope: "global" | "local", watchRoot: string, configPath: string) {
    logger.log("config.watcher_started", {
      watchScope: scope,
      watchRoot,
      configPath,
    })
  }

  function logWatcherClosed(scope: "global" | "local", watchRoot: string, configPath: string) {
    logger.log("config.watcher_closed", {
      watchScope: scope,
      watchRoot,
      configPath,
    })
  }

  function logReloadSuccess(entry: CachedRootConfigEntry, changedLayer: "global" | "local") {
    if (!entry.snapshot) {
      return
    }

    logger.log("config.snapshot_promoted", {
      watchScope: changedLayer,
      localConfigPath: entry.localConfigPath,
      version: entry.snapshot.version,
    })
  }

  function logReloadFailure(
    entry: CachedRootConfigEntry,
    changedLayer: "global" | "local",
    error: unknown,
  ) {
    logger.log("config.reload_failed", {
      watchScope: changedLayer,
      localConfigPath: entry.localConfigPath,
      errorMessage: error instanceof Error ? error.message : String(error),
      version: entry.snapshot?.version,
    })
  }

  function createWatcher(
    scope: "global" | "local",
    watchRoot: string,
    configPath: string,
    onChange: () => void,
  ) {
    const watcher = chokidar.watch(resolve(configPath), {
      ignoreInitial: true,
      awaitWriteFinish: {
        stabilityThreshold: WATCH_STABILITY_MS,
        pollInterval: WATCH_POLL_INTERVAL_MS,
      },
    })
    const ready = new Promise<void>((resolveReady) => {
      watcher.once("ready", resolveReady)
    })

    watcher.on("add", onChange)
    watcher.on("change", onChange)
    watcher.on("unlink", onChange)
    watcher.on("error", (error) => {
      logger.log("config.watcher_degraded", {
        watchScope: scope,
        watchRoot: resolve(watchRoot),
        configPath: resolve(configPath),
        errorMessage: error instanceof Error ? error.message : String(error),
      })
    })

    return { watcher, ready }
  }

  function getOrCreateEntry(cwd: string) {
    const resolvedCwd = resolve(cwd)
    const localConfigPath = getLocalConfigPath(resolvedCwd)
    const existingEntry = entries.get(localConfigPath)
    if (existingEntry) {
      return existingEntry
    }

    const entry: CachedRootConfigEntry = {
      cwd: resolvedCwd,
      localRoot: getGoddardLocalDir(resolvedCwd),
      localConfigPath,
      snapshot: null,
      watcher: null,
      watcherReady: null,
      reloadTask: null,
      debounceHandle: null,
    }
    entries.set(localConfigPath, entry)
    return entry
  }

  function scheduleReload(entry: CachedRootConfigEntry, changedLayer: "global" | "local") {
    if (closed) {
      return
    }

    if (entry.debounceHandle) {
      clearTimeout(entry.debounceHandle)
    }

    entry.debounceHandle = setTimeout(() => {
      entry.debounceHandle = null
      void refreshEntry(entry, changedLayer).catch(() => {})
    }, RELOAD_DEBOUNCE_MS)
  }

  async function refreshEntry(entry: CachedRootConfigEntry, changedLayer: "global" | "local") {
    const runRefresh = async () => {
      try {
        const nextConfig = await readMergedRootConfig(entry.cwd)
        entry.snapshot = {
          ...nextConfig,
          version: (entry.snapshot?.version ?? 0) + 1,
          loadedAt: new Date().toISOString(),
        }
        logReloadSuccess(entry, changedLayer)
        return entry.snapshot
      } catch (error) {
        logReloadFailure(entry, changedLayer, error)
        if (entry.snapshot) {
          return entry.snapshot
        }
        throw error
      }
    }

    const previousTask = entry.reloadTask ?? Promise.resolve(entry.snapshot)
    const nextTask = previousTask.then(runRefresh, runRefresh)
    entry.reloadTask = nextTask
    void nextTask.finally(() => {
      if (entry.reloadTask === nextTask) {
        entry.reloadTask = null
      }
    })

    return nextTask
  }

  async function ensureEntryWatching(entry: CachedRootConfigEntry) {
    if (closed) {
      return
    }

    if (!entry.watcher) {
      const localWatcher = createWatcher("local", entry.localRoot, entry.localConfigPath, () => {
        scheduleReload(entry, "local")
      })
      entry.watcher = localWatcher.watcher
      entry.watcherReady = Promise.all([globalWatcherReady, localWatcher.ready]).then(() => {})
      logWatcherStarted("local", entry.localRoot, entry.localConfigPath)
    }

    await entry.watcherReady
  }

  return {
    async getRootConfig(cwd: string = process.cwd()) {
      const entry = getOrCreateEntry(cwd)
      await ensureEntryWatching(entry)
      if (entry.snapshot) {
        return entry.snapshot
      }

      return refreshEntry(entry, "local")
    },

    getLastKnownRootConfig(cwd: string = process.cwd()) {
      return entries.get(getLocalConfigPath(resolve(cwd)))?.snapshot ?? null
    },

    async ensureWatching(cwd: string) {
      await ensureEntryWatching(getOrCreateEntry(cwd))
    },

    async close() {
      if (closed) {
        return
      }
      closed = true

      await globalWatcher.close().catch(() => {})
      logWatcherClosed("global", globalRoot, globalConfigPath)

      for (const entry of entries.values()) {
        if (entry.debounceHandle) {
          clearTimeout(entry.debounceHandle)
          entry.debounceHandle = null
        }
        if (entry.watcher) {
          await entry.watcher.close().catch(() => {})
          logWatcherClosed("local", entry.localRoot, entry.localConfigPath)
        }
      }
      entries.clear()
    },
  } satisfies ConfigManager
}
