import { signal } from "@preact/signals"
import { castProtected, sigma, type Immutable, type Protected } from "preact-sigma"
import { useEffect, useRef } from "preact/hooks"
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
export function captureAppStateSnapshot(appState: AppState) {
  return {
    appearance: sigma.captureState(appState.appearance as unknown as Appearance),
    navigation: sigma.captureState(appState.navigation as unknown as Navigation),
    projectContext: sigma.captureState(appState.projectContext as unknown as ProjectContext),
    projectRegistry: sigma.captureState(appState.projectRegistry as unknown as ProjectRegistry),
    workbenchTabSet: sigma.captureState(appState.workbenchTabSet as unknown as WorkbenchTabSet),
  }
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
  appState: AppState,
  writeSnapshot: SnapshotWriter<PersistedAppStateSnapshot>,
  options: ObserveSnapshotOptions = {},
) {
  return observeSnapshot(
    () => captureAppStateSnapshot(appState),
    writeSnapshot,
    (queueSnapshotWrite) => [
      sigma.subscribe(appState.appearance as unknown as Appearance, queueSnapshotWrite),
      sigma.subscribe(appState.navigation as unknown as Navigation, queueSnapshotWrite),
      sigma.subscribe(appState.projectContext as unknown as ProjectContext, queueSnapshotWrite),
      sigma.subscribe(appState.projectRegistry as unknown as ProjectRegistry, queueSnapshotWrite),
      sigma.subscribe(appState.workbenchTabSet as unknown as WorkbenchTabSet, queueSnapshotWrite),
    ],
    options,
  )
}

/** Creates the app's singleton Sigma models before async daemon state restoration. */
export function createAppState() {
  const appearance = castProtected(
    new Appearance({
      mode: "system",
      highContrast: false,
    }),
  )

  appearance.applyDocumentAppearance()

  return {
    appearance,
    inbox: castProtected(new Inbox(goddardSdk.inbox)),
    navigation: castProtected(new Navigation()),
    projectContext: castProtected(new ProjectContext()),
    projectRegistry: castProtected(new ProjectRegistry()),
    shortcutRegistry,
    workbenchTabSet: castProtected(new WorkbenchTabSet()),
  } satisfies AppState
}

/** Owns app-state restoration, setup, and persistence for the provider boundary. */
export function useAppState() {
  const appStateRef = useRef<AppState | null>(null)
  if (!appStateRef.current) {
    appStateRef.current = createAppState()
  }
  const appState = appStateRef.current

  useEffect(() => {
    let isDisposed = false
    let appStateObserver: SnapshotObserver | null = null

    function syncProjectContext() {
      ;(appState.projectContext as unknown as ProjectContext).syncProjects(
        appState.projectRegistry.projectList.map((project) => project.path),
      )
    }

    function startAppStateObserver() {
      if (isDisposed || appStateObserver) {
        return
      }

      appStateObserver = observeAppStateSnapshot(
        appState,
        (snapshot) => {
          return desktopHost.writeAppStateSnapshot(snapshot)
        },
        {
          onWriteError(error) {
            console.error("Failed to save app state.", error)
          },
        },
      )
    }

    void desktopHost.loadAppStateSnapshot().then(
      (snapshot) => {
        if (isDisposed) {
          return
        }

        if (snapshot) {
          const persistedSnapshot = snapshot as PersistedAppStateSnapshot
          sigma.replaceState(
            appState.appearance as unknown as Appearance,
            persistedSnapshot.appearance,
          )
          sigma.replaceState(
            appState.navigation as unknown as Navigation,
            persistedSnapshot.navigation,
          )
          sigma.replaceState(
            appState.projectContext as unknown as ProjectContext,
            persistedSnapshot.projectContext,
          )
          sigma.replaceState(
            appState.projectRegistry as unknown as ProjectRegistry,
            persistedSnapshot.projectRegistry,
          )
          sigma.replaceState(
            appState.workbenchTabSet as unknown as WorkbenchTabSet,
            persistedSnapshot.workbenchTabSet,
          )
          ;(appState.appearance as unknown as Appearance).applyDocumentAppearance()
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

    return () => {
      isDisposed = true
      void appStateObserver?.stop()
    }
  }, [appState])

  useEffect(() => {
    return (appState.appearance as unknown as Appearance).setup()
  }, [appState])

  useEffect(() => {
    let isDisposed = false
    let shortcutKeymapObserver: SnapshotObserver | null = null
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    function startShortcutKeymapObserver() {
      if (isDisposed || shortcutKeymapObserver) {
        return
      }

      shortcutKeymapObserver = observeSnapshot(
        () => ({
          version: SHORTCUT_KEYMAP_FILE_VERSION as ShortcutKeymapFile["version"],
          selectedProfileId: shortcutRegistry.selectedProfileId,
          overrides: shortcutRegistry.overrides,
        }),
        async (snapshot) => {
          await desktopHost.writeShortcutKeymap(snapshot)
          shortcutPersistenceErrors.value = {
            ...shortcutPersistenceErrors.value,
            loadError: null,
            writeError: null,
          }
        },
        (queueSnapshotWrite) => [sigma.subscribe(shortcutRegistry, queueSnapshotWrite)],
        {
          onWriteError(error) {
            shortcutPersistenceErrors.value = {
              ...shortcutPersistenceErrors.value,
              writeError: `Failed to save shortcut keymap: ${getErrorMessage(error)}`,
            }
          },
        },
      )
    }

    void desktopHost.loadShortcutKeymap().then(
      (snapshot) => {
        if (isDisposed) {
          return
        }

        if (snapshot) {
          shortcutRegistry.applyKeymapSnapshot(snapshot.selectedProfileId, snapshot.overrides)
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
      void shortcutKeymapObserver?.stop()
      cleanupShortcutRegistry()
    }
  }, [])

  return {
    appearance: appState.appearance,
    inbox: appState.inbox,
    navigation: appState.navigation,
    projectContext: appState.projectContext,
    projectRegistry: appState.projectRegistry,
    shortcutRegistry,
    workbenchTabSet: appState.workbenchTabSet,
  } satisfies AppState
}
