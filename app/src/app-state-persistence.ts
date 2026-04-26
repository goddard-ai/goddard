import { signal } from "@preact/signals"
import { useSigma, type Immutable, type Protected } from "preact-sigma"
import {
  hydrate,
  persist,
  restoreSync,
  type PersistCodec,
  type PersistStore,
} from "preact-sigma/persist"
import { useEffect, useMemo } from "preact/hooks"

import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { isAppearanceMode } from "./appearance/theme.ts"
import { desktopHost } from "./desktop-host.ts"
import { isNavigationItemId, Navigation, type NavigationState } from "./navigation.ts"
import { ProjectContext, type ProjectContextState } from "./projects/project-context.ts"
import {
  ProjectRegistry,
  type ProjectRecord,
  type ProjectRegistryState,
} from "./projects/project-registry.ts"
import { createDefaultShortcutKeymapFile } from "./shared/shortcut-keymap.ts"
import {
  shortcutRegistry,
  type ShortcutRegistry,
  type ShortcutRegistryState,
} from "./shortcuts/shortcut-registry.ts"
import { createWorkspaceStorageStore } from "./support/workspace-storage.ts"
import { isWorkbenchDetailTabKind } from "./workbench-tab-registry.ts"
import {
  WORKBENCH_PRIMARY_TAB,
  WorkbenchTabSet,
  type WorkbenchDetailTab,
  type WorkbenchTabSetState,
} from "./workbench-tab-set.ts"

const APPEARANCE_STORAGE_KEY = "goddard.app.appearance.v2"
const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v3"
const PROJECT_CONTEXT_STORAGE_KEY = "goddard.app.project-context.v2"
const PROJECT_REGISTRY_STORAGE_KEY = "goddard.app.projects.v2"
const WORKBENCH_TABS_STORAGE_KEY = "goddard.app.workbench-tabs.v4"
const SHORTCUT_REGISTRY_STORAGE_KEY = "goddard.app.shortcuts.v1"

type AppearanceStoredState = Pick<AppearanceState, "mode" | "highContrast">
type NavigationStoredState = Pick<NavigationState, "selectedNavId">
type ShortcutStoredState = Pick<ShortcutRegistryState, "selectedProfileId" | "overrides">

/** Context-ready app model bundle produced by the persistence lifecycle hook. */
export type PersistentAppState = {
  appearance: Protected<Appearance>
  navigation: Protected<Navigation>
  projectContext: Protected<ProjectContext>
  projectRegistry: Protected<ProjectRegistry>
  shortcutRegistry: ShortcutRegistry
  workbenchTabSet: Protected<WorkbenchTabSet>
}

/** Non-Sigma persistence status for the keyboard shortcut settings UI. */
export const shortcutPersistenceErrors = signal({
  loadError: null as string | null,
  writeError: null as string | null,
})

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value))
}

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message
  }

  return typeof error === "string" && error.length > 0 ? error : "Unknown error."
}

function isProjectRecord(value: unknown): value is ProjectRecord {
  return isPlainRecord(value) && typeof value.path === "string" && typeof value.name === "string"
}

function isStoredWorkbenchTab(tab: unknown): tab is WorkbenchDetailTab & {
  payload: any
} {
  return (
    isPlainRecord(tab) &&
    typeof tab.id === "string" &&
    typeof tab.title === "string" &&
    typeof tab.dirty === "boolean" &&
    typeof tab.kind === "string" &&
    isWorkbenchDetailTabKind(tab.kind)
  )
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

/** Creates the app's singleton Sigma models and restores their persisted state. */
export function createRestoredAppState(initialAppearanceState: AppearanceState) {
  const appearance = new Appearance(initialAppearanceState)
  const navigation = new Navigation()
  const projectContext = new ProjectContext()
  const projectRegistry = new ProjectRegistry()
  const workbenchTabSet = new WorkbenchTabSet()

  restoreSync(appearance, {
    key: APPEARANCE_STORAGE_KEY,
    store: createWorkspaceStorageStore<AppearanceStoredState>(),
    codec: appearanceCodec,
  })
  restoreSync(navigation, {
    key: NAVIGATION_STORAGE_KEY,
    store: createWorkspaceStorageStore<NavigationStoredState>(),
    codec: navigationCodec,
  })
  restoreSync(projectContext, {
    key: PROJECT_CONTEXT_STORAGE_KEY,
    store: createWorkspaceStorageStore<Immutable<ProjectContextState>>(),
    codec: projectContextCodec,
  })
  restoreSync(projectRegistry, {
    key: PROJECT_REGISTRY_STORAGE_KEY,
    store: createWorkspaceStorageStore<Immutable<ProjectRegistryState>>(),
    codec: projectRegistryCodec,
  })
  restoreSync(workbenchTabSet, {
    key: WORKBENCH_TABS_STORAGE_KEY,
    store: createWorkspaceStorageStore<Immutable<WorkbenchTabSetState>>(),
    codec: workbenchTabSetCodec,
  })

  appearance.applyDocumentAppearance()

  return {
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    workbenchTabSet,
  }
}

const appearanceCodec = {
  version: 1,
  encode(state) {
    return {
      mode: state.mode,
      highContrast: state.highContrast,
    }
  },
  decode(stored, { baseState }) {
    if (!isPlainRecord(stored)) {
      return { ...baseState }
    }

    return {
      ...baseState,
      mode: isAppearanceMode(stored.mode) ? stored.mode : baseState.mode,
      highContrast: stored.highContrast === true,
    }
  },
} satisfies PersistCodec<AppearanceState, AppearanceStoredState>

const navigationCodec = {
  version: 1,
  encode(state) {
    return {
      selectedNavId: state.selectedNavId,
    }
  },
  decode(stored, { baseState }) {
    if (!isPlainRecord(stored) || !isNavigationItemId(stored.selectedNavId)) {
      return { ...baseState }
    }

    return {
      selectedNavId: stored.selectedNavId,
    }
  },
} satisfies PersistCodec<NavigationState, NavigationStoredState>

const projectContextCodec = {
  version: 1,
  encode(state) {
    return {
      activeProjectPath: state.activeProjectPath,
      recentProjectPaths: state.recentProjectPaths,
    }
  },
  decode(stored, { baseState }) {
    if (!isPlainRecord(stored)) {
      return {
        activeProjectPath: baseState.activeProjectPath,
        recentProjectPaths: [...baseState.recentProjectPaths],
      }
    }

    return {
      activeProjectPath:
        typeof stored.activeProjectPath === "string" ? stored.activeProjectPath : null,
      recentProjectPaths: Array.isArray(stored.recentProjectPaths)
        ? [...new Set(stored.recentProjectPaths.filter((path) => typeof path === "string"))]
        : [...baseState.recentProjectPaths],
    }
  },
} satisfies PersistCodec<ProjectContextState>

const projectRegistryCodec = {
  version: 1,
  encode(state) {
    return {
      projectsByPath: state.projectsByPath,
      orderedProjectPaths: state.orderedProjectPaths,
    }
  },
  decode(stored, { baseState }) {
    if (!isPlainRecord(stored) || !isPlainRecord(stored.projectsByPath)) {
      return {
        projectsByPath: { ...baseState.projectsByPath },
        orderedProjectPaths: [...baseState.orderedProjectPaths],
      }
    }

    const projectsByPath = Object.fromEntries(
      Object.entries(stored.projectsByPath).filter((entry): entry is [string, ProjectRecord] => {
        return isProjectRecord(entry[1]) && entry[0] === entry[1].path
      }),
    )
    const orderedProjectPaths = Array.isArray(stored.orderedProjectPaths)
      ? stored.orderedProjectPaths.filter((path) => {
          return typeof path === "string" && Boolean(projectsByPath[path])
        })
      : Object.keys(projectsByPath)

    return {
      projectsByPath,
      orderedProjectPaths,
    }
  },
} satisfies PersistCodec<ProjectRegistryState>

const workbenchTabSetCodec = {
  version: 1,
  encode(state) {
    return {
      tabs: state.tabs,
      orderedTabIds: state.orderedTabIds,
      activeTabId: state.activeTabId,
      recency: state.recency,
    }
  },
  decode(stored, { baseState }) {
    if (!isPlainRecord(stored) || !isPlainRecord(stored.tabs)) {
      return {
        tabs: { ...baseState.tabs },
        orderedTabIds: [...baseState.orderedTabIds],
        activeTabId: baseState.activeTabId,
        recency: [...baseState.recency],
      }
    }

    const tabs = Object.fromEntries(
      Object.entries(stored.tabs).filter((entry): entry is [string, WorkbenchDetailTab] => {
        const tab = entry[1]
        return isStoredWorkbenchTab(tab) && entry[0] === tab.id
      }),
    )
    const orderedTabIds = Array.isArray(stored.orderedTabIds)
      ? stored.orderedTabIds.filter((tabId) => typeof tabId === "string" && Boolean(tabs[tabId]))
      : Object.keys(tabs)
    const storedActiveTabId =
      typeof stored.activeTabId === "string" ? stored.activeTabId : WORKBENCH_PRIMARY_TAB.id
    const activeTabId =
      storedActiveTabId === WORKBENCH_PRIMARY_TAB.id || Boolean(tabs[storedActiveTabId])
        ? storedActiveTabId
        : WORKBENCH_PRIMARY_TAB.id

    return {
      tabs,
      orderedTabIds,
      activeTabId,
      recency: Array.isArray(stored.recency)
        ? stored.recency.filter((tabId) => typeof tabId === "string" && Boolean(tabs[tabId]))
        : [...baseState.recency],
    }
  },
} satisfies PersistCodec<WorkbenchTabSetState>

const shortcutRegistryStore = {
  async get() {
    const response = await desktopHost.readShortcutKeymap()
    const keymap = response.keymap ?? createDefaultShortcutKeymapFile()
    shortcutPersistenceErrors.value = {
      ...shortcutPersistenceErrors.value,
      loadError: response.error,
    }

    return {
      savedAt: Date.now(),
      value: {
        selectedProfileId: keymap.profile,
        overrides: keymap.overrides,
      },
      version: keymap.version,
    }
  },
  async set(_key, record) {
    await desktopHost.writeShortcutKeymap({
      version: 1,
      profile: record.value.selectedProfileId,
      overrides: record.value.overrides,
    })
    shortcutPersistenceErrors.value = {
      ...shortcutPersistenceErrors.value,
      writeError: null,
    }
  },
  async delete() {
    await desktopHost.writeShortcutKeymap(createDefaultShortcutKeymapFile())
  },
} satisfies PersistStore<ShortcutStoredState>

/** Reads the persisted appearance state before the first render and applies it to the document. */
export function getInitialAppearanceState() {
  const appearance = new Appearance(createDefaultAppearanceState())
  restoreSync(appearance, {
    key: APPEARANCE_STORAGE_KEY,
    store: createWorkspaceStorageStore<AppearanceStoredState>(),
    codec: appearanceCodec,
  })
  appearance.applyDocumentAppearance()

  return cloneAppearanceState(appearance)
}

/** Owns app-state restoration, setup, and persistence for the provider boundary. */
export function usePersistentAppState(initialAppearanceState: AppearanceState) {
  const appState = useMemo(
    () => createRestoredAppState(initialAppearanceState),
    [initialAppearanceState],
  )
  const appearance = useSigma(() => appState.appearance, {
    deps: [appState.appearance],
  })
  const navigation = useSigma(() => appState.navigation, [appState.navigation])
  const projectContext = useSigma(() => appState.projectContext, [appState.projectContext])
  const projectRegistry = useSigma(() => appState.projectRegistry, [appState.projectRegistry])
  const workbenchTabSet = useSigma(() => appState.workbenchTabSet, [appState.workbenchTabSet])

  useEffect(() => {
    const persistenceHandles = [
      persist(appState.appearance, {
        key: APPEARANCE_STORAGE_KEY,
        store: createWorkspaceStorageStore<AppearanceStoredState>(),
        codec: appearanceCodec,
      }),
      persist(appState.navigation, {
        key: NAVIGATION_STORAGE_KEY,
        store: createWorkspaceStorageStore<NavigationStoredState>(),
        codec: navigationCodec,
      }),
      persist(appState.projectContext, {
        key: PROJECT_CONTEXT_STORAGE_KEY,
        store: createWorkspaceStorageStore<Immutable<ProjectContextState>>(),
        codec: projectContextCodec,
      }),
      persist(appState.projectRegistry, {
        key: PROJECT_REGISTRY_STORAGE_KEY,
        store: createWorkspaceStorageStore<Immutable<ProjectRegistryState>>(),
        codec: projectRegistryCodec,
      }),
      persist(appState.workbenchTabSet, {
        key: WORKBENCH_TABS_STORAGE_KEY,
        store: createWorkspaceStorageStore<Immutable<WorkbenchTabSetState>>(),
        codec: workbenchTabSetCodec,
      }),
    ]
    const shortcutPersistence = hydrate(shortcutRegistry, {
      key: SHORTCUT_REGISTRY_STORAGE_KEY,
      store: shortcutRegistryStore,
      pick: ["selectedProfileId", "overrides"],
      onWriteError(error) {
        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          writeError: `Failed to save shortcut overrides: ${getErrorMessage(error)}`,
        }
      },
    })
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    void shortcutPersistence.restored.then(
      () => {
        shortcutRegistry.rebindRuntime()
      },
      (error) => {
        shortcutPersistenceErrors.value = {
          ...shortcutPersistenceErrors.value,
          loadError: `Failed to read shortcut keymap: ${getErrorMessage(error)}`,
        }
      },
    )

    appState.projectContext.syncProjects(
      appState.projectRegistry.projectList.map((project) => project.path),
    )

    return () => {
      for (const handle of persistenceHandles) {
        void handle.stop()
      }

      void shortcutPersistence.stop()
      cleanupShortcutRegistry()
    }
  }, [appState])

  return {
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    shortcutRegistry,
    workbenchTabSet,
  }
}
