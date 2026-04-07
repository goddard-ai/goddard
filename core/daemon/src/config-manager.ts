import { existsSync, watch, type FSWatcher } from "node:fs"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/paths/node"
import type { UserConfig } from "@goddard-ai/schema/config"
import { basename, dirname, resolve } from "node:path"
import { createDaemonLogger } from "./logging.ts"
import { readMergedRootConfig } from "./resolvers/config.ts"

const WATCH_RELOAD_SETTLE_MS = 50
const WATCH_RELOAD_RETRY_MS = 50
const MAX_WATCH_RELOAD_RETRIES = 2

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

type WatchScope = "global" | "local"
type WatchMode = "root" | "parent"

type WatchedConfigState = {
  scope: WatchScope
  rootDir: string
  parentDir: string
  configPath: string
  watchMode: WatchMode | null
  watchedDir: string | null
  watcher: FSWatcher | null
}

type CachedRootConfigEntry = {
  cwd: string
  localRoot: string
  localConfigPath: string
  watchState: WatchedConfigState
  snapshot: RootConfigSnapshot | null
  reloadTask: Promise<RootConfigSnapshot | null> | null
  debounceHandle: ReturnType<typeof setTimeout> | null
}

/** Creates the daemon-owned config manager for merged persisted root-config snapshots. */
export function createConfigManager() {
  const logger = createDaemonLogger()
  const entries = new Map<string, CachedRootConfigEntry>()
  const globalRoot = resolve(getGoddardGlobalDir())
  const globalConfigPath = resolve(getGlobalConfigPath())
  const globalWatchState = createWatchedConfigState("global", globalRoot, globalConfigPath)
  let closed = false

  ensureWatchTarget(globalWatchState, () => {
    for (const entry of entries.values()) {
      scheduleReload(entry, "global")
    }
  })

  function createWatchedConfigState(scope: WatchScope, rootDir: string, configPath: string) {
    return {
      scope,
      rootDir,
      parentDir: resolve(dirname(rootDir)),
      configPath,
      watchMode: null,
      watchedDir: null,
      watcher: null,
    } satisfies WatchedConfigState
  }

  function resolveWatchTarget(state: WatchedConfigState) {
    if (existsSync(state.rootDir)) {
      return {
        watchMode: "root" as const,
        watchedDir: state.rootDir,
      }
    }

    return {
      watchMode: "parent" as const,
      watchedDir: state.parentDir,
    }
  }

  function closeWatchTarget(state: WatchedConfigState) {
    if (!state.watcher || !state.watchedDir) {
      return
    }

    try {
      state.watcher.close()
    } catch {
      // Best-effort shutdown only.
    }

    logger.log("config.watcher_closed", {
      watchScope: state.scope,
      watchRoot: state.watchedDir,
      configPath: state.configPath,
    })
    state.watcher = null
    state.watchedDir = null
    state.watchMode = null
  }

  function shouldHandleWatchEvent(
    state: WatchedConfigState,
    watchMode: WatchMode,
    filename: string | Buffer | null,
  ) {
    if (filename == null) {
      return true
    }

    const normalizedName = filename.toString()
    return watchMode === "root"
      ? normalizedName === basename(state.configPath)
      : normalizedName === basename(state.rootDir)
  }

  function ensureWatchTarget(state: WatchedConfigState, onChange: () => void) {
    if (closed) {
      return
    }

    const nextTarget = resolveWatchTarget(state)
    if (
      state.watcher &&
      state.watchMode === nextTarget.watchMode &&
      state.watchedDir === nextTarget.watchedDir
    ) {
      return
    }

    closeWatchTarget(state)

    const watcher = watch(nextTarget.watchedDir, (eventType, filename) => {
      if (closed || state.watcher !== watcher) {
        return
      }

      const previousWatchMode = state.watchMode ?? nextTarget.watchMode
      const previousWatchedDir = state.watchedDir ?? nextTarget.watchedDir

      ensureWatchTarget(state, onChange)

      if (eventType !== "change" && eventType !== "rename") {
        return
      }

      if (
        previousWatchMode !== state.watchMode ||
        previousWatchedDir !== state.watchedDir ||
        shouldHandleWatchEvent(state, previousWatchMode, filename ?? null)
      ) {
        onChange()
      }
    })

    watcher.on("error", (error) => {
      if (state.watcher !== watcher) {
        return
      }

      logger.log("config.watcher_degraded", {
        watchScope: state.scope,
        watchRoot: state.watchedDir ?? nextTarget.watchedDir,
        configPath: state.configPath,
        errorMessage: error instanceof Error ? error.message : String(error),
      })
    })

    state.watcher = watcher
    state.watchedDir = nextTarget.watchedDir
    state.watchMode = nextTarget.watchMode

    logger.log("config.watcher_started", {
      watchScope: state.scope,
      watchRoot: nextTarget.watchedDir,
      configPath: state.configPath,
    })
  }

  function getOrCreateEntry(cwd: string) {
    const resolvedCwd = resolve(cwd)
    const localRoot = resolve(getGoddardLocalDir(resolvedCwd))
    const localConfigPath = resolve(getLocalConfigPath(resolvedCwd))
    const existingEntry = entries.get(localConfigPath)
    if (existingEntry) {
      return existingEntry
    }

    const entry: CachedRootConfigEntry = {
      cwd: resolvedCwd,
      localRoot,
      localConfigPath,
      watchState: createWatchedConfigState("local", localRoot, localConfigPath),
      snapshot: null,
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
      void refreshEntry(entry, changedLayer, {
        watcherTriggered: true,
      }).catch(() => {})
    }, WATCH_RELOAD_SETTLE_MS)
  }

  async function refreshEntry(
    entry: CachedRootConfigEntry,
    changedLayer: "global" | "local",
    options: { watcherTriggered?: boolean } = {},
  ) {
    const runRefresh = async () => {
      let attemptsRemaining = options.watcherTriggered ? MAX_WATCH_RELOAD_RETRIES : 0

      while (true) {
        try {
          const nextConfig = await readMergedRootConfig(entry.cwd)
          entry.snapshot = {
            ...nextConfig,
            version: (entry.snapshot?.version ?? 0) + 1,
            loadedAt: new Date().toISOString(),
          }

          logger.log("config.snapshot_promoted", {
            watchScope: changedLayer,
            localConfigPath: entry.localConfigPath,
            version: entry.snapshot.version,
          })
          return entry.snapshot
        } catch (error) {
          if (attemptsRemaining > 0) {
            attemptsRemaining -= 1
            await Bun.sleep(WATCH_RELOAD_RETRY_MS)
            continue
          }

          logger.log("config.reload_failed", {
            watchScope: changedLayer,
            localConfigPath: entry.localConfigPath,
            errorMessage: error instanceof Error ? error.message : String(error),
            version: entry.snapshot?.version,
          })
          if (entry.snapshot) {
            return entry.snapshot
          }
          throw error
        }
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
    ensureWatchTarget(entry.watchState, () => {
      scheduleReload(entry, "local")
    })
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
      return entries.get(resolve(getLocalConfigPath(resolve(cwd))))?.snapshot ?? null
    },

    async ensureWatching(cwd: string) {
      await ensureEntryWatching(getOrCreateEntry(cwd))
    },

    async close() {
      if (closed) {
        return
      }
      closed = true

      closeWatchTarget(globalWatchState)

      for (const entry of entries.values()) {
        if (entry.debounceHandle) {
          clearTimeout(entry.debounceHandle)
          entry.debounceHandle = null
        }
        closeWatchTarget(entry.watchState)
      }
      entries.clear()
    },
  } satisfies ConfigManager
}
