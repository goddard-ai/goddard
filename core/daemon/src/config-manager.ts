import { existsSync, watch, type FSWatcher } from "node:fs"
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises"
import { basename, dirname, resolve } from "node:path"
import {
  getGlobalConfigPath,
  getGoddardGlobalDir,
  getGoddardLocalDir,
  getLocalConfigPath,
} from "@goddard-ai/paths/node"
import { getErrorMessage, isObject, omit } from "radashi"

import { buildRootConfigSchema } from "./config-schema.ts"
import type { ConfigReloadFailedEvent } from "./events.ts"
import { createDebug, createLogger } from "./logging.ts"
import { readMergedRootConfig, type RootConfig } from "./resolvers/config.ts"

const WATCH_RELOAD_SETTLE_MS = 50
const WATCH_RELOAD_RETRY_MS = 50
const MAX_WATCH_RELOAD_RETRIES = 2
// Parent directory watches can miss the single event that reports a config root was created.
const WATCH_ROOT_DISCOVERY_PROBE_MS = 250

/** One validated merged root-config snapshot owned by the daemon config manager. */
export type RootConfigSnapshot = {
  globalRoot: string
  localRoot: string
  config: RootConfig
  version: number
  loadedAt: string
}

/** Daemon-owned contract for serving hot-reloadable persisted root-config snapshots. */
export interface ConfigManager {
  getRootConfig: (cwd?: string) => Promise<RootConfigSnapshot>
  getLastKnownRootConfig: (cwd?: string) => RootConfigSnapshot | null
  getGlobalConfig: () => Promise<Record<string, unknown>>
  updateGlobalConfig: (
    update: (config: Readonly<Record<string, unknown>>) => Record<string, unknown>,
  ) => Promise<Record<string, unknown>>
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
  rootDiscoveryProbe: ReturnType<typeof setTimeout> | null
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

type CreateConfigManagerOptions = {
  onReloadFailed?: (event: ConfigReloadFailedEvent) => void | Promise<void>
}

/** Creates the daemon-owned config manager for merged persisted root-config snapshots. */
export function createConfigManager(options: CreateConfigManagerOptions = {}) {
  const logger = createLogger()
  const debug = createDebug("config.watch")
  const { onReloadFailed } = options
  const entries = new Map<string, CachedRootConfigEntry>()
  const globalRoot = resolve(getGoddardGlobalDir())
  const globalConfigPath = resolve(getGlobalConfigPath())
  const globalWatchState = createWatchedConfigState("global", globalRoot, globalConfigPath)
  let globalUpdateTask = Promise.resolve()
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
      rootDiscoveryProbe: null,
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
    if (state.rootDiscoveryProbe) {
      clearTimeout(state.rootDiscoveryProbe)
      state.rootDiscoveryProbe = null
    }

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

  function ensureRootDiscoveryProbe(state: WatchedConfigState, onChange: () => void) {
    if (state.watchMode !== "parent" || state.rootDiscoveryProbe) {
      return
    }

    state.rootDiscoveryProbe = setTimeout(() => {
      state.rootDiscoveryProbe = null

      if (closed || state.watchMode !== "parent") {
        return
      }

      ensureWatchTarget(state, onChange)

      if (state.watchMode === "parent") {
        ensureRootDiscoveryProbe(state, onChange)
        return
      }

      onChange()
    }, WATCH_ROOT_DISCOVERY_PROBE_MS)

    state.rootDiscoveryProbe.unref()
  }

  function shouldHandleWatchEvent(
    state: WatchedConfigState,
    eventType: string,
    watchMode: WatchMode,
    filename: string | Buffer | null,
  ) {
    if (watchMode === "root" && eventType === "rename") {
      return true
    }

    if (filename == null) {
      return true
    }

    const normalizedName = filename.toString()
    return watchMode === "root"
      ? normalizedName === basename(state.configPath)
      : normalizedName === basename(state.rootDir)
  }

  function isMissingWatchTarget(error: unknown) {
    return typeof error === "object" && error != null && "code" in error && error.code === "ENOENT"
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

    debug("config.watch.target_resolved", {
      watchScope: state.scope,
      watchMode: nextTarget.watchMode,
      watchRoot: nextTarget.watchedDir,
      configPath: state.configPath,
    })

    let watcher: FSWatcher
    try {
      watcher = watch(nextTarget.watchedDir, (eventType, filename) => {
        if (closed || state.watcher !== watcher) {
          return
        }

        const previousWatchMode = state.watchMode ?? nextTarget.watchMode
        const previousWatchedDir = state.watchedDir ?? nextTarget.watchedDir

        ensureWatchTarget(state, onChange)

        if (eventType !== "change" && eventType !== "rename") {
          debug("config.watch.event_ignored", {
            watchScope: state.scope,
            watchMode: previousWatchMode,
            watchRoot: previousWatchedDir,
            eventType,
            filename: filename?.toString(),
            reason: "unsupported_event",
          })
          return
        }

        const shouldReload =
          previousWatchMode !== state.watchMode ||
          previousWatchedDir !== state.watchedDir ||
          shouldHandleWatchEvent(state, eventType, previousWatchMode, filename ?? null)
        debug(shouldReload ? "config.watch.event_matched" : "config.watch.event_ignored", {
          watchScope: state.scope,
          watchMode: previousWatchMode,
          watchRoot: previousWatchedDir,
          eventType,
          filename: filename?.toString(),
          reason: shouldReload ? undefined : "unrelated_path",
        })
        if (shouldReload) {
          onChange()
        }
      })
    } catch (error) {
      if (closed) {
        return
      }

      logger.log("config.watcher_degraded", {
        watchScope: state.scope,
        watchRoot: nextTarget.watchedDir,
        configPath: state.configPath,
        errorMessage: getErrorMessage(error),
      })

      if (isMissingWatchTarget(error)) {
        // fs.watch can lose the root to CI cleanup after target resolution; retrying lets
        // the next pass fall back to watching the parent directory.
        if (nextTarget.watchMode === "root" && !existsSync(nextTarget.watchedDir)) {
          ensureWatchTarget(state, onChange)
        }
        return
      }

      throw error
    }

    watcher.on("error", (error) => {
      if (state.watcher !== watcher) {
        return
      }

      logger.log("config.watcher_degraded", {
        watchScope: state.scope,
        watchRoot: state.watchedDir ?? nextTarget.watchedDir,
        configPath: state.configPath,
        errorMessage: getErrorMessage(error),
      })
    })

    state.watcher = watcher
    state.watchedDir = nextTarget.watchedDir
    state.watchMode = nextTarget.watchMode
    ensureRootDiscoveryProbe(state, onChange)

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

    debug("config.watch.reload_scheduled", {
      watchScope: changedLayer,
      localConfigPath: entry.localConfigPath,
      version: entry.snapshot?.version,
      replacedPendingReload: Boolean(entry.debounceHandle),
    })
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
          debug("config.watch.reload_started", {
            watchScope: changedLayer,
            localConfigPath: entry.localConfigPath,
            attemptsRemaining,
          })
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

          const reloadFailedEvent = {
            watchScope: changedLayer,
            cwd: entry.cwd,
            localConfigPath: entry.localConfigPath,
            errorMessage: getErrorMessage(error),
            version: entry.snapshot?.version,
          } satisfies ConfigReloadFailedEvent
          if (onReloadFailed) {
            await onReloadFailed(reloadFailedEvent)
          } else {
            logger.log("config.reload_failed", reloadFailedEvent)
          }
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

  async function readGlobalConfigForUpdate() {
    let rawConfig: unknown

    try {
      rawConfig = JSON.parse(await readFile(globalConfigPath, "utf8"))
    } catch (error) {
      if (isMissingWatchTarget(error)) {
        return {
          schemaReference: undefined,
          config: {},
        }
      }
      throw error
    }

    if (!isObject(rawConfig) || Array.isArray(rawConfig)) {
      throw new Error(`Global config at ${globalConfigPath} must be a JSON object.`)
    }
    const rawConfigRecord = rawConfig as Record<string, unknown>

    return {
      schemaReference:
        typeof rawConfigRecord.$schema === "string" ? rawConfigRecord.$schema : undefined,
      config: omit(rawConfigRecord, ["$schema"]),
    }
  }

  async function writeGlobalConfig(
    update: (config: Readonly<Record<string, unknown>>) => Record<string, unknown>,
  ) {
    const { schemaReference, config } = await readGlobalConfigForUpdate()
    const rootConfigSchema = buildRootConfigSchema()
    const currentConfig = rootConfigSchema.parse(config)
    const nextConfig = rootConfigSchema.parse(update(currentConfig))
    const serializedConfig = schemaReference
      ? { $schema: schemaReference, ...nextConfig }
      : nextConfig
    const temporaryPath = `${globalConfigPath}.${crypto.randomUUID()}.tmp`

    await mkdir(globalRoot, { recursive: true })
    try {
      await writeFile(temporaryPath, `${JSON.stringify(serializedConfig, null, 2)}\n`, {
        encoding: "utf8",
        mode: 0o600,
      })
      await rename(temporaryPath, globalConfigPath)
    } finally {
      await rm(temporaryPath, { force: true }).catch(() => {})
    }

    await Promise.all([...entries.values()].map((entry) => refreshEntry(entry, "global")))

    return nextConfig
  }

  async function getGlobalConfig() {
    await globalUpdateTask
    const { config } = await readGlobalConfigForUpdate()
    return buildRootConfigSchema().parse(config)
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

    getGlobalConfig,

    updateGlobalConfig(update) {
      const nextTask = globalUpdateTask.then(
        () => writeGlobalConfig(update),
        () => writeGlobalConfig(update),
      )
      globalUpdateTask = nextTask.then(
        () => undefined,
        () => undefined,
      )
      return nextTask
    },

    async ensureWatching(cwd: string) {
      await ensureEntryWatching(getOrCreateEntry(cwd))
    },

    async close() {
      if (closed) {
        return
      }
      closed = true

      await globalUpdateTask

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
