import { signal } from "@preact/signals"
import { sigma, useSigma, type Immutable, type Protected } from "preact-sigma"
import { useEffect, useMemo } from "preact/hooks"
import { getErrorMessage } from "radashi"

import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { desktopHost } from "./desktop-host.ts"
import { Navigation, type NavigationState } from "./navigation.ts"
import { ProjectContext, type ProjectContextState } from "./projects/project-context.ts"
import { ProjectRegistry, type ProjectRegistryState } from "./projects/project-registry.ts"
import { SHORTCUT_KEYMAP_FILE_VERSION, type ShortcutKeymapFile } from "./shared/shortcut-keymap.ts"
import { shortcutRegistry, type ShortcutRegistry } from "./shortcuts/shortcut-registry.ts"
import { WorkbenchTabSet, type WorkbenchTabSetState } from "./workbench-tab-set.ts"

const APP_STATE_STORAGE_KEY = "goddard.app.state.v1"
const APP_STATE_STORAGE_SCOPE = {
  scopeKind: "window",
  scopeId: "primary",
} as const
const APP_STATE_RECORD_VERSION = 1
const APP_STATE_WRITE_DEBOUNCE_MS = 250

/** Raw app Sigma model bundle before async daemon state restoration. */
export type RestoredAppModels = {
  appearance: Appearance
  navigation: Navigation
  projectContext: ProjectContext
  projectRegistry: ProjectRegistry
  workbenchTabSet: WorkbenchTabSet
}

/** Context-ready app model bundle produced by the persistence lifecycle hook. */
export type PersistentAppModels = {
  appearance: Protected<Appearance>
  navigation: Protected<Navigation>
  projectContext: Protected<ProjectContext>
  projectRegistry: Protected<ProjectRegistry>
  shortcutRegistry: ShortcutRegistry
  workbenchTabSet: Protected<WorkbenchTabSet>
}

/** Persisted Sigma state captured and restored as one daemon-owned app state record. */
export type PersistedAppStateSnapshot = {
  appearance: Immutable<AppearanceState>
  navigation: Immutable<NavigationState>
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

/** Non-Sigma persistence status surfaced in settings UI. */
export const shortcutPersistenceErrors = signal({
  loadError: null as string | null,
  writeError: null as string | null,
})

/** Captures the current committed app Sigma state as one daemon-persisted snapshot. */
export function captureAppStateSnapshot(appModels: RestoredAppModels) {
  return {
    appearance: sigma.captureState(appModels.appearance),
    navigation: sigma.captureState(appModels.navigation),
    projectContext: sigma.captureState(appModels.projectContext),
    projectRegistry: sigma.captureState(appModels.projectRegistry),
    workbenchTabSet: sigma.captureState(appModels.workbenchTabSet),
  }
}

function applyAppStateSnapshot(appModels: RestoredAppModels, snapshot: PersistedAppStateSnapshot) {
  sigma.replaceState(appModels.appearance, snapshot.appearance)
  sigma.replaceState(appModels.navigation, snapshot.navigation)
  sigma.replaceState(appModels.projectContext, snapshot.projectContext)
  sigma.replaceState(appModels.projectRegistry, snapshot.projectRegistry)
  sigma.replaceState(appModels.workbenchTabSet, snapshot.workbenchTabSet)

  appModels.appearance.applyDocumentAppearance()
}

function cancelTimer(timer: ReturnType<typeof setTimeout> | null) {
  if (timer !== null) {
    clearTimeout(timer)
  }
}

/**
 * Observes one or more Sigma sources and writes the latest captured snapshot after changes settle.
 */
function observeSnapshot<TSnapshot>(
  captureSnapshot: () => TSnapshot,
  writeSnapshot: SnapshotWriter<TSnapshot>,
  subscribeSnapshots: (queueSnapshotWrite: () => void) => Array<() => void>,
  options: ObserveSnapshotOptions = {},
) {
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
        await writeSnapshot(snapshot)
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

    pendingSnapshot = captureSnapshot()
    scheduleWrite()
  }

  const unsubscribe = subscribeSnapshots(queueSnapshotWrite)

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

/** Observes committed app Sigma changes and writes the latest combined snapshot. */
export function observeAppStateSnapshot(
  appModels: RestoredAppModels,
  writeSnapshot: SnapshotWriter<PersistedAppStateSnapshot>,
  options: ObserveSnapshotOptions = {},
) {
  return observeSnapshot(
    () => captureAppStateSnapshot(appModels),
    writeSnapshot,
    (queueSnapshotWrite) => [
      sigma.subscribe(appModels.appearance, queueSnapshotWrite),
      sigma.subscribe(appModels.navigation, queueSnapshotWrite),
      sigma.subscribe(appModels.projectContext, queueSnapshotWrite),
      sigma.subscribe(appModels.projectRegistry, queueSnapshotWrite),
      sigma.subscribe(appModels.workbenchTabSet, queueSnapshotWrite),
    ],
    options,
  )
}

/** Captures the user-editable shortcut keymap as the app-only JSON file shape. */
function captureShortcutKeymapSnapshot() {
  return {
    version: SHORTCUT_KEYMAP_FILE_VERSION as ShortcutKeymapFile["version"],
    selectedProfileId: shortcutRegistry.selectedProfileId,
    overrides: shortcutRegistry.overrides,
  }
}

/** Applies one persisted shortcut keymap and refreshes the live keyboard runtime. */
function applyShortcutKeymapSnapshot(snapshot: ShortcutKeymapFile) {
  shortcutRegistry.applyKeymapSnapshot(snapshot.selectedProfileId, snapshot.overrides)
}

/** Observes shortcut keymap edits and writes the latest app-only keymap snapshot. */
function observeShortcutKeymapSnapshot(
  writeSnapshot: SnapshotWriter<ShortcutKeymapFile>,
  options: ObserveSnapshotOptions = {},
) {
  return observeSnapshot(
    captureShortcutKeymapSnapshot,
    writeSnapshot,
    (queueSnapshotWrite) => [sigma.subscribe(shortcutRegistry, queueSnapshotWrite)],
    options,
  )
}

/** Creates the app's singleton Sigma models before async daemon state restoration. */
export function createRestoredAppModels() {
  const appearance = new Appearance({
    mode: "system",
    highContrast: false,
  })
  const navigation = new Navigation()
  const projectContext = new ProjectContext()
  const projectRegistry = new ProjectRegistry()
  const workbenchTabSet = new WorkbenchTabSet()

  appearance.applyDocumentAppearance()

  return {
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    workbenchTabSet,
  } satisfies RestoredAppModels
}

async function loadPersistedAppStateSnapshot() {
  const response = await desktopHost.sdk.appState.get({
    key: APP_STATE_STORAGE_KEY,
    ...APP_STATE_STORAGE_SCOPE,
  })

  return (response.state?.value ?? null) as PersistedAppStateSnapshot | null
}

async function writePersistedAppStateSnapshot(snapshot: PersistedAppStateSnapshot) {
  await desktopHost.sdk.appState.set({
    key: APP_STATE_STORAGE_KEY,
    ...APP_STATE_STORAGE_SCOPE,
    record: {
      version: APP_STATE_RECORD_VERSION,
      savedAt: Date.now(),
      value: snapshot,
    },
  })
}

async function loadPersistedShortcutKeymapSnapshot() {
  return await desktopHost.loadShortcutKeymap()
}

async function writePersistedShortcutKeymapSnapshot(snapshot: ShortcutKeymapFile) {
  await desktopHost.writeShortcutKeymap(snapshot)
  shortcutPersistenceErrors.value = {
    ...shortcutPersistenceErrors.value,
    loadError: null,
    writeError: null,
  }
}

/** Owns app-state restoration, setup, and persistence for the provider boundary. */
export function usePersistentAppModels() {
  const appModels = useMemo(() => createRestoredAppModels(), [])
  const appearance = useSigma(() => appModels.appearance, {
    deps: [appModels.appearance],
  })
  const navigation = useSigma(() => appModels.navigation, [appModels.navigation])
  const projectContext = useSigma(() => appModels.projectContext, [appModels.projectContext])
  const projectRegistry = useSigma(() => appModels.projectRegistry, [appModels.projectRegistry])
  const workbenchTabSet = useSigma(() => appModels.workbenchTabSet, [appModels.workbenchTabSet])

  useEffect(() => {
    let isDisposed = false
    let appStateObserver: SnapshotObserver | null = null
    let shortcutKeymapObserver: SnapshotObserver | null = null
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    function syncProjectContext() {
      appModels.projectContext.syncProjects(
        appModels.projectRegistry.projectList.map((project) => project.path),
      )
    }

    function startAppStateObserver() {
      if (isDisposed || appStateObserver) {
        return
      }

      appStateObserver = observeAppStateSnapshot(appModels, writePersistedAppStateSnapshot, {
        onWriteError(error) {
          console.error("Failed to save app state.", error)
        },
      })
    }

    function startShortcutKeymapObserver() {
      if (isDisposed || shortcutKeymapObserver) {
        return
      }

      shortcutKeymapObserver = observeShortcutKeymapSnapshot(writePersistedShortcutKeymapSnapshot, {
        onWriteError(error) {
          shortcutPersistenceErrors.value = {
            ...shortcutPersistenceErrors.value,
            writeError: `Failed to save shortcut keymap: ${getErrorMessage(error)}`,
          }
        },
      })
    }

    void loadPersistedAppStateSnapshot().then(
      (snapshot) => {
        if (isDisposed) {
          return
        }

        if (snapshot) {
          applyAppStateSnapshot(appModels, snapshot)
        }

        startAppStateObserver()
        syncProjectContext()
      },
      (error) => {
        if (isDisposed) {
          return
        }

        startAppStateObserver()
        syncProjectContext()
        console.error("Failed to load app state.", error)
      },
    )

    void loadPersistedShortcutKeymapSnapshot().then(
      (snapshot) => {
        if (isDisposed) {
          return
        }

        if (snapshot) {
          applyShortcutKeymapSnapshot(snapshot)
        } else {
          shortcutRegistry.rebindRuntime()
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

        shortcutRegistry.rebindRuntime()
        startShortcutKeymapObserver()
        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          loadError: `Failed to load shortcut keymap: ${getErrorMessage(error)}`,
        }
      },
    )

    return () => {
      isDisposed = true
      void appStateObserver?.stop()
      void shortcutKeymapObserver?.stop()
      cleanupShortcutRegistry()
    }
  }, [appModels])

  return {
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    shortcutRegistry,
    workbenchTabSet,
  } satisfies PersistentAppModels
}
