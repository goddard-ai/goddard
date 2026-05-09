import { signal } from "@preact/signals"
import { sigma, useSigma, type Immutable, type Protected } from "preact-sigma"
import { useEffect, useMemo } from "preact/hooks"
import { getErrorMessage } from "radashi"

import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { desktopHost } from "./desktop-host.ts"
import { Inbox } from "./inbox/model.ts"
import { Navigation, type NavigationState } from "./navigation.ts"
import { ProjectContext, type ProjectContextState } from "./projects/project-context.ts"
import { ProjectRegistry, type ProjectRegistryState } from "./projects/project-registry.ts"
import { goddardSdk } from "./sdk.ts"
import type { AppStateSnapshot } from "./shared/app-state.ts"
import { SHORTCUT_KEYMAP_FILE_VERSION, type ShortcutKeymapFile } from "./shared/shortcut-keymap.ts"
import { shortcutRegistry, type ShortcutRegistry } from "./shortcuts/shortcut-registry.ts"
import { WorkbenchTabSet, type WorkbenchTabSetState } from "./workbench-tab-set.ts"

const APP_STATE_WRITE_DEBOUNCE_MS = 250

/** Raw app Sigma model bundle before async daemon state restoration. */
type AppStateModels = {
  appearance: Appearance
  inbox: Inbox
  navigation: Navigation
  projectContext: ProjectContext
  projectRegistry: ProjectRegistry
  workbenchTabSet: WorkbenchTabSet
}

/** Context-ready app model bundle produced by the app lifecycle hook. */
export type AppState = {
  appearance: Protected<Appearance>
  inbox: Protected<Inbox>
  navigation: Protected<Navigation>
  projectContext: Protected<ProjectContext>
  projectRegistry: Protected<ProjectRegistry>
  shortcutRegistry: ShortcutRegistry
  workbenchTabSet: Protected<WorkbenchTabSet>
}

/** Persisted Sigma state captured and restored through the Bun-host app state file. */
export type PersistedAppStateSnapshot = AppStateSnapshot & {
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

/** Captures the current committed app Sigma state as one app-owned persisted snapshot. */
export function captureAppStateSnapshot(appState: AppStateModels) {
  return {
    appearance: sigma.captureState(appState.appearance),
    navigation: sigma.captureState(appState.navigation),
    projectContext: sigma.captureState(appState.projectContext),
    projectRegistry: sigma.captureState(appState.projectRegistry),
    workbenchTabSet: sigma.captureState(appState.workbenchTabSet),
  }
}

function applyAppStateSnapshot(appState: AppStateModels, snapshot: PersistedAppStateSnapshot) {
  sigma.replaceState(appState.appearance, snapshot.appearance)
  sigma.replaceState(appState.navigation, snapshot.navigation)
  sigma.replaceState(appState.projectContext, snapshot.projectContext)
  sigma.replaceState(appState.projectRegistry, snapshot.projectRegistry)
  sigma.replaceState(appState.workbenchTabSet, snapshot.workbenchTabSet)

  appState.appearance.applyDocumentAppearance()
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
  appState: AppStateModels,
  writeSnapshot: SnapshotWriter<PersistedAppStateSnapshot>,
  options: ObserveSnapshotOptions = {},
) {
  return observeSnapshot(
    () => captureAppStateSnapshot(appState),
    writeSnapshot,
    (queueSnapshotWrite) => [
      sigma.subscribe(appState.appearance, queueSnapshotWrite),
      sigma.subscribe(appState.navigation, queueSnapshotWrite),
      sigma.subscribe(appState.projectContext, queueSnapshotWrite),
      sigma.subscribe(appState.projectRegistry, queueSnapshotWrite),
      sigma.subscribe(appState.workbenchTabSet, queueSnapshotWrite),
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
export function createAppState() {
  const appearance = new Appearance({
    mode: "system",
    highContrast: false,
  })
  const inbox = new Inbox(goddardSdk.inbox)
  const navigation = new Navigation()
  const projectContext = new ProjectContext()
  const projectRegistry = new ProjectRegistry()
  const workbenchTabSet = new WorkbenchTabSet()

  appearance.applyDocumentAppearance()

  return {
    appearance,
    inbox,
    navigation,
    projectContext,
    projectRegistry,
    workbenchTabSet,
  } satisfies AppStateModels
}

async function loadPersistedAppStateSnapshot() {
  return (await desktopHost.loadAppStateSnapshot()) as PersistedAppStateSnapshot | null
}

async function writePersistedAppStateSnapshot(snapshot: PersistedAppStateSnapshot) {
  await desktopHost.writeAppStateSnapshot(snapshot)
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
export function useAppState() {
  const appState = useMemo(() => createAppState(), [])
  const appearance = useSigma(() => appState.appearance, {
    deps: [appState.appearance],
  })
  const inbox = useSigma(() => appState.inbox, [appState.inbox])
  const navigation = useSigma(() => appState.navigation, [appState.navigation])
  const projectContext = useSigma(() => appState.projectContext, [appState.projectContext])
  const projectRegistry = useSigma(() => appState.projectRegistry, [appState.projectRegistry])
  const workbenchTabSet = useSigma(() => appState.workbenchTabSet, [appState.workbenchTabSet])

  useEffect(() => {
    let isDisposed = false
    let appStateObserver: SnapshotObserver | null = null
    let shortcutKeymapObserver: SnapshotObserver | null = null
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    function syncProjectContext() {
      appState.projectContext.syncProjects(
        appState.projectRegistry.projectList.map((project) => project.path),
      )
    }

    function startAppStateObserver() {
      if (isDisposed || appStateObserver) {
        return
      }

      appStateObserver = observeAppStateSnapshot(appState, writePersistedAppStateSnapshot, {
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
          applyAppStateSnapshot(appState, snapshot)
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
  }, [appState])

  return {
    appearance,
    inbox,
    navigation,
    projectContext,
    projectRegistry,
    shortcutRegistry,
    workbenchTabSet,
  } satisfies AppState
}
