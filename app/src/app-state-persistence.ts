import { signal } from "@preact/signals"
import { sigma, useSigma, type Immutable, type Protected } from "preact-sigma"
import { useEffect, useMemo } from "preact/hooks"

import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { desktopHost } from "./desktop-host.ts"
import { Navigation, type NavigationState } from "./navigation.ts"
import { ProjectContext, type ProjectContextState } from "./projects/project-context.ts"
import { ProjectRegistry, type ProjectRegistryState } from "./projects/project-registry.ts"
import {
  shortcutRegistry,
  type ShortcutRegistry,
  type ShortcutRegistryState,
} from "./shortcuts/shortcut-registry.ts"
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

/** Persisted Sigma state captured and restored as one daemon-owned app settings record. */
export type PersistedAppStateSnapshot = {
  appearance: Immutable<AppearanceState>
  navigation: Immutable<NavigationState>
  projectContext: Immutable<ProjectContextState>
  projectRegistry: Immutable<ProjectRegistryState>
  shortcutRegistry: Immutable<ShortcutRegistryState>
  workbenchTabSet: Immutable<WorkbenchTabSetState>
}

/** Async persistence writer used by the app-state snapshot observer. */
type AppStateSnapshotWriter = (snapshot: PersistedAppStateSnapshot) => Promise<void>

/** Runtime handle for the app-state snapshot observation loop. */
type AppStateSnapshotObserver = {
  flush(): Promise<void>
  stop(): Promise<void>
}

/** Optional behavior for the app-state snapshot observation loop. */
type ObserveAppStateSnapshotOptions = {
  debounceMs?: number
  onWriteError?: (error: unknown) => void
}

/** Non-Sigma persistence status surfaced in settings UI. */
export const shortcutPersistenceErrors = signal({
  loadError: null as string | null,
  writeError: null as string | null,
})

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message
  }

  return typeof error === "string" && error.length > 0 ? error : "Unknown error."
}

function cloneAppearanceState(state: AppearanceState) {
  return {
    mode: state.mode,
    highContrast: state.highContrast,
  }
}

function createDefaultAppearanceState() {
  return {
    mode: "system",
    highContrast: false,
  } satisfies AppearanceState
}

/** Captures the current committed app Sigma state as one daemon-persisted snapshot. */
export function captureAppStateSnapshot(appModels: RestoredAppModels) {
  return {
    appearance: sigma.captureState(appModels.appearance),
    navigation: sigma.captureState(appModels.navigation),
    projectContext: sigma.captureState(appModels.projectContext),
    projectRegistry: sigma.captureState(appModels.projectRegistry),
    shortcutRegistry: sigma.captureState(shortcutRegistry),
    workbenchTabSet: sigma.captureState(appModels.workbenchTabSet),
  }
}

function applyAppStateSnapshot(appModels: RestoredAppModels, snapshot: PersistedAppStateSnapshot) {
  sigma.replaceState(appModels.appearance, snapshot.appearance)
  sigma.replaceState(appModels.navigation, snapshot.navigation)
  sigma.replaceState(appModels.projectContext, snapshot.projectContext)
  sigma.replaceState(appModels.projectRegistry, snapshot.projectRegistry)
  sigma.replaceState(shortcutRegistry, snapshot.shortcutRegistry)
  sigma.replaceState(appModels.workbenchTabSet, snapshot.workbenchTabSet)

  appModels.appearance.applyDocumentAppearance()
  shortcutRegistry.rebindRuntime()
}

function cancelTimer(timer: ReturnType<typeof setTimeout> | null) {
  if (timer !== null) {
    clearTimeout(timer)
  }
}

/** Observes committed app Sigma changes and writes the latest combined snapshot. */
export function observeAppStateSnapshot(
  appModels: RestoredAppModels,
  writeSnapshot: AppStateSnapshotWriter,
  options: ObserveAppStateSnapshotOptions = {},
) {
  const debounceMs = options.debounceMs ?? APP_STATE_WRITE_DEBOUNCE_MS
  let isStopped = false
  let pendingSnapshot: PersistedAppStateSnapshot | null = null
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

    pendingSnapshot = captureAppStateSnapshot(appModels)
    scheduleWrite()
  }

  const unsubscribe = [
    sigma.subscribe(appModels.appearance, queueSnapshotWrite),
    sigma.subscribe(appModels.navigation, queueSnapshotWrite),
    sigma.subscribe(appModels.projectContext, queueSnapshotWrite),
    sigma.subscribe(appModels.projectRegistry, queueSnapshotWrite),
    sigma.subscribe(shortcutRegistry, queueSnapshotWrite),
    sigma.subscribe(appModels.workbenchTabSet, queueSnapshotWrite),
  ]

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

/** Creates the app's singleton Sigma models before async daemon state restoration. */
export function createRestoredAppModels(initialAppearanceState: AppearanceState) {
  const appearance = new Appearance(initialAppearanceState)
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
  const response = await desktopHost.sdk.appSettings.get({
    key: APP_STATE_STORAGE_KEY,
    ...APP_STATE_STORAGE_SCOPE,
  })

  return (response.setting?.value ?? null) as PersistedAppStateSnapshot | null
}

async function writePersistedAppStateSnapshot(snapshot: PersistedAppStateSnapshot) {
  await desktopHost.sdk.appSettings.set({
    key: APP_STATE_STORAGE_KEY,
    ...APP_STATE_STORAGE_SCOPE,
    record: {
      version: APP_STATE_RECORD_VERSION,
      savedAt: Date.now(),
      value: snapshot,
    },
  })
  shortcutPersistenceErrors.value = {
    ...shortcutPersistenceErrors.value,
    writeError: null,
  }
}

/** Creates the initial appearance state before async daemon persistence is available. */
export function getInitialAppearanceState() {
  const appearance = new Appearance(createDefaultAppearanceState())
  appearance.applyDocumentAppearance()

  return cloneAppearanceState(appearance)
}

/** Owns app-state restoration, setup, and persistence for the provider boundary. */
export function usePersistentAppModels(initialAppearanceState: AppearanceState) {
  const appModels = useMemo(
    () => createRestoredAppModels(initialAppearanceState),
    [initialAppearanceState],
  )
  const appearance = useSigma(() => appModels.appearance, {
    deps: [appModels.appearance],
  })
  const navigation = useSigma(() => appModels.navigation, [appModels.navigation])
  const projectContext = useSigma(() => appModels.projectContext, [appModels.projectContext])
  const projectRegistry = useSigma(() => appModels.projectRegistry, [appModels.projectRegistry])
  const workbenchTabSet = useSigma(() => appModels.workbenchTabSet, [appModels.workbenchTabSet])

  useEffect(() => {
    let isDisposed = false
    let appStateObserver: AppStateSnapshotObserver | null = null
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
          shortcutPersistenceErrors.value = {
            ...shortcutPersistenceErrors.value,
            writeError: `Failed to save app settings: ${getErrorMessage(error)}`,
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
        } else {
          shortcutRegistry.rebindRuntime()
        }

        startAppStateObserver()
        syncProjectContext()
        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          loadError: null,
        }
      },
      (error) => {
        if (isDisposed) {
          return
        }

        startAppStateObserver()
        syncProjectContext()
        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          loadError: `Failed to load app settings: ${getErrorMessage(error)}`,
        }
      },
    )

    return () => {
      isDisposed = true
      void appStateObserver?.stop()
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
