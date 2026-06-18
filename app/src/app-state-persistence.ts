import { signal } from "@preact/signals"
import { castProtected, sigma, useSetup, type Immutable } from "preact-sigma"
import { useEffect, useState } from "preact/hooks"
import { getErrorMessage } from "radashi"

import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { readBootAppearanceSnapshot } from "./appearance/boot-appearance.ts"
import { CommandContext } from "./commands/command-context.ts"
import { desktopHost } from "./desktop-host.ts"
import { MainTab, type MainTabState } from "./main-tab.ts"
import { ProjectContext, type ProjectContextState } from "./projects/project-context.ts"
import { ProjectRegistry, type ProjectRegistryState } from "./projects/project-registry.ts"
import { startSessionLifecycleSubscription } from "./sessions/lifecycle.ts"
import { SHORTCUT_KEYMAP_FILE_VERSION } from "./shared/shortcut-keymap.ts"
import { ShortcutRegistry } from "./shortcuts/shortcut-registry.ts"
import { WorkbenchTabCache } from "./workbench-tab-cache.ts"
import { WorkbenchTabSet, type WorkbenchTabSetState } from "./workbench-tab-set.ts"

const APP_STATE_WRITE_DEBOUNCE_MS = 250

type AppStateHydrationStatus = "pending" | "ready"

/** Context-ready app model bundle produced by the app lifecycle hook. */
export type AppState = ReturnType<typeof useAppState>

/** Persisted Sigma state captured and restored through the Bun-host app state file. */
export type AppStateSnapshot = {
  appearance: Immutable<AppearanceState>
  mainTab: Immutable<MainTabState>
  projectContext: Immutable<ProjectContextState>
  projectRegistry: Immutable<ProjectRegistryState>
  workbenchTabSet: Immutable<WorkbenchTabSetState>
}

/** Async persistence writer used by debounced snapshot observers. */
type SnapshotWriter<TSnapshot> = (snapshot: TSnapshot) => Promise<void>

/** Runtime handle for a debounced persistence observation loop. */
type SnapshotObserver = {
  flush(): Promise<void>
  stop(): Promise<void>
}

/** Optional behavior for debounced persistence observation loops. */
type ObserveSnapshotOptions = {
  debounceMs?: number
  onWriteError?: (error: unknown) => void
}

/** Readable dependency bundle for one debounced snapshot observer. */
type ObserveSnapshotProps<TSnapshot> = {
  captureSnapshot: () => TSnapshot
  writeSnapshot: SnapshotWriter<TSnapshot>
  subscribeSnapshots: (queueSnapshotWrite: () => void) => Array<() => void>
  options?: ObserveSnapshotOptions
}

/** Non-Sigma persistence status surfaced in settings UI. */
export const shortcutPersistenceErrors = signal({
  loadError: null as string | null,
  writeError: null as string | null,
})

function cancelTimer(timer: ReturnType<typeof setTimeout> | null) {
  if (timer !== null) {
    clearTimeout(timer)
  }
}

/**
 * Observes one or more Sigma sources and writes the latest captured snapshot after changes settle.
 */
function observeSnapshot<const TSnapshot>(props: ObserveSnapshotProps<TSnapshot>) {
  const options = props.options ?? {}
  const debounceMs = options.debounceMs ?? APP_STATE_WRITE_DEBOUNCE_MS
  let isStopped = false
  let pendingSnapshot: TSnapshot | null = null
  let writeTimer: ReturnType<typeof setTimeout> | null = null
  let runningWrite: Promise<void> | null = null

  function cancelScheduledWrite() {
    cancelTimer(writeTimer)
    writeTimer = null
  }

  async function drainPendingWrites() {
    if (runningWrite) {
      return runningWrite
    }

    cancelScheduledWrite()
    runningWrite = (async () => {
      while (pendingSnapshot && !isStopped) {
        const snapshot = pendingSnapshot
        pendingSnapshot = null
        await props.writeSnapshot(snapshot)
      }
    })()

    try {
      await runningWrite
    } finally {
      runningWrite = null

      if (pendingSnapshot && !isStopped) {
        startBackgroundWrite()
      }
    }
  }

  function startBackgroundWrite() {
    void drainPendingWrites().catch((error) => {
      options.onWriteError?.(error)
    })
  }

  function scheduleWrite() {
    if (isStopped) {
      return
    }

    cancelScheduledWrite()
    writeTimer = setTimeout(() => {
      writeTimer = null
      startBackgroundWrite()
    }, debounceMs)
  }

  function queueSnapshotWrite() {
    if (isStopped) {
      return
    }

    pendingSnapshot = props.captureSnapshot()
    scheduleWrite()
  }

  const unsubscribe = props.subscribeSnapshots(queueSnapshotWrite)

  return {
    async flush() {
      cancelScheduledWrite()

      if (!pendingSnapshot) {
        await runningWrite
        return
      }

      await drainPendingWrites()
    },
    async stop() {
      if (isStopped) {
        await runningWrite
        return
      }

      isStopped = true
      cancelScheduledWrite()
      pendingSnapshot = null

      for (const unsubscribeSnapshot of unsubscribe) {
        unsubscribeSnapshot()
      }

      await runningWrite
    },
  }
}

/** Owns app-state restoration, setup, and persistence for the provider boundary. */
export function useAppState() {
  const [hydrationStatus, setHydrationStatus] = useState<AppStateHydrationStatus>("pending")
  const [appState] = useState(() => {
    const bootAppearance = readBootAppearanceSnapshot()
    const workbenchTabCache = new WorkbenchTabCache()
    const mainTab = new MainTab()
    const projectRegistry = new ProjectRegistry()
    const workbenchTabSet = new WorkbenchTabSet({
      onCloseTab: (tabId) => {
        workbenchTabCache.disposeTab(tabId)
      },
    })
    const commandContext = new CommandContext({
      mainTab,
      workbenchTabSet,
    })

    return {
      appearance: new Appearance({
        mode: bootAppearance.mode,
        highContrast: bootAppearance.highContrast,
      }),
      commandContext,
      mainTab,
      projectContext: new ProjectContext({
        projectRegistry,
        workbenchTabSet,
      }),
      projectRegistry,
      shortcutRegistry: new ShortcutRegistry({
        runtime: commandContext.runtime,
      }),
      workbenchTabCache,
      workbenchTabSet,
    }
  })

  useEffect(() => {
    let isDisposed = false
    let appStateObserver: SnapshotObserver | null = null

    function startAppStateObserver() {
      if (isDisposed || appStateObserver) {
        return
      }

      appStateObserver = observeSnapshot({
        captureSnapshot: () => ({
          appearance: sigma.captureState(appState.appearance),
          mainTab: sigma.captureState(appState.mainTab),
          projectContext: sigma.captureState(appState.projectContext),
          projectRegistry: sigma.captureState(appState.projectRegistry),
          workbenchTabSet: sigma.captureState(appState.workbenchTabSet),
        }),
        writeSnapshot: (snapshot) => {
          return desktopHost.writeAppStateSnapshot(snapshot)
        },
        subscribeSnapshots: (queueSnapshotWrite) => [
          sigma.subscribe(appState.appearance, queueSnapshotWrite),
          sigma.subscribe(appState.mainTab, queueSnapshotWrite),
          sigma.subscribe(appState.projectContext, queueSnapshotWrite),
          sigma.subscribe(appState.projectRegistry, queueSnapshotWrite),
          sigma.subscribe(appState.workbenchTabSet, queueSnapshotWrite),
        ],
        options: {
          onWriteError(error) {
            console.error("Failed to save app state.", error)
          },
        },
      })
    }

    void desktopHost.loadAppStateSnapshot<AppStateSnapshot>().then(
      (snapshot) => {
        if (isDisposed) {
          return
        }

        if (snapshot) {
          sigma.replaceState(appState.appearance, snapshot.appearance)
          sigma.replaceState(appState.mainTab, snapshot.mainTab)
          sigma.replaceState(appState.projectContext, snapshot.projectContext)
          sigma.replaceState(appState.projectRegistry, snapshot.projectRegistry)
          sigma.replaceState(appState.workbenchTabSet, snapshot.workbenchTabSet)
        }

        setHydrationStatus("ready")
        startAppStateObserver()
      },
      (error) => {
        if (isDisposed) {
          return
        }

        setHydrationStatus("ready")
        startAppStateObserver()
        console.error("Failed to load app state.", error)
      },
    )

    return () => {
      isDisposed = true
      void appStateObserver?.stop()
    }
  }, [appState])

  useSetup(() => {
    return [
      appState.appearance.setup(),
      appState.commandContext.setup(),
      appState.projectContext.setup(),
      startSessionLifecycleSubscription(),
      appState.shortcutRegistry.setup(),
      () => {
        appState.workbenchTabCache.disposeAll()
      },
    ]
  }, [appState])

  useEffect(() => {
    let isDisposed = false
    let shortcutKeymapObserver: SnapshotObserver | null = null

    function startShortcutKeymapObserver() {
      if (isDisposed || shortcutKeymapObserver) {
        return
      }

      shortcutKeymapObserver = observeSnapshot({
        captureSnapshot: () => ({
          version: SHORTCUT_KEYMAP_FILE_VERSION,
          selectedProfileId: appState.shortcutRegistry.selectedProfileId,
          overrides: appState.shortcutRegistry.overrides,
        }),
        writeSnapshot: async (snapshot) => {
          await desktopHost.writeShortcutKeymap(snapshot)
          shortcutPersistenceErrors.value = {
            ...shortcutPersistenceErrors.value,
            loadError: null,
            writeError: null,
          }
        },
        subscribeSnapshots: (queueSnapshotWrite) => [
          sigma.subscribe(appState.shortcutRegistry, queueSnapshotWrite),
        ],
        options: {
          onWriteError(error) {
            shortcutPersistenceErrors.value = {
              ...shortcutPersistenceErrors.value,
              writeError: `Failed to save shortcut keymap: ${getErrorMessage(error)}`,
            }
          },
        },
      })
    }

    void desktopHost.loadShortcutKeymap().then(
      (snapshot) => {
        if (isDisposed) {
          return
        }

        if (snapshot) {
          appState.shortcutRegistry.applyKeymapSnapshot(
            snapshot.selectedProfileId,
            snapshot.overrides,
          )
        } else {
          appState.shortcutRegistry.rebindRuntime()
        }

        startShortcutKeymapObserver()
        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          loadError: null,
        }
      },
      (error) => {
        if (isDisposed) {
          return
        }

        appState.shortcutRegistry.rebindRuntime()
        startShortcutKeymapObserver()

        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          loadError: `Failed to load shortcut keymap: ${getErrorMessage(error)}`,
        }
      },
    )

    return () => {
      isDisposed = true
      void shortcutKeymapObserver?.stop()
    }
  }, [])

  return {
    appearance: castProtected(appState.appearance),
    mainTab: castProtected(appState.mainTab),
    projectContext: castProtected(appState.projectContext),
    projectRegistry: castProtected(appState.projectRegistry),
    shortcutRegistry: castProtected(appState.shortcutRegistry),
    workbenchTabCache: appState.workbenchTabCache,
    workbenchTabSet: castProtected(appState.workbenchTabSet),
    hydrationStatus,
  }
}
