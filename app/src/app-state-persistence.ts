import { signal } from "@preact/signals"
import { useSigma, type Protected } from "preact-sigma"
import { hydrate, persist, restoreSync, type PersistStore } from "preact-sigma/persist"
import { useEffect, useMemo } from "preact/hooks"

import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { desktopHost } from "./desktop-host.ts"
import { createWorkspaceStorageStore } from "./lib/workspace-storage.ts"
import { Navigation, type NavigationState } from "./navigation.ts"
import { ProjectContext, type ProjectContextState } from "./projects/project-context.ts"
import { ProjectRegistry, type ProjectRegistryState } from "./projects/project-registry.ts"
import { createDefaultShortcutKeymapFile } from "./shared/shortcut-keymap.ts"
import {
  shortcutRegistry,
  type ShortcutRegistry,
  type ShortcutRegistryState,
} from "./shortcuts/shortcut-registry.ts"
import { WorkbenchTabSet, type WorkbenchTabSetState } from "./workbench-tab-set.ts"

const APPEARANCE_STORAGE_KEY = "goddard.app.appearance.v2"
const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v3"
const PROJECT_CONTEXT_STORAGE_KEY = "goddard.app.project-context.v2"
const PROJECT_REGISTRY_STORAGE_KEY = "goddard.app.projects.v2"
const WORKBENCH_TABS_STORAGE_KEY = "goddard.app.workbench-tabs.v4"
const SHORTCUT_REGISTRY_STORAGE_KEY = "goddard.app.shortcuts.v1"

/** Raw app Sigma model bundle after synchronous state restoration. */
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

/** Non-Sigma persistence status for the keyboard shortcut settings UI. */
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

/** Creates the app's singleton Sigma models and restores their persisted state. */
export function createRestoredAppModels(initialAppearanceState: AppearanceState) {
  const appearance = new Appearance(initialAppearanceState)
  const navigation = new Navigation()
  const projectContext = new ProjectContext()
  const projectRegistry = new ProjectRegistry()
  const workbenchTabSet = new WorkbenchTabSet()

  restoreSync(appearance, {
    key: APPEARANCE_STORAGE_KEY,
    store: createWorkspaceStorageStore<AppearanceState>(),
  })
  restoreSync(navigation, {
    key: NAVIGATION_STORAGE_KEY,
    store: createWorkspaceStorageStore<NavigationState>(),
  })
  restoreSync(projectContext, {
    key: PROJECT_CONTEXT_STORAGE_KEY,
    store: createWorkspaceStorageStore<ProjectContextState>(),
  })
  restoreSync(projectRegistry, {
    key: PROJECT_REGISTRY_STORAGE_KEY,
    store: createWorkspaceStorageStore<ProjectRegistryState>(),
  })
  restoreSync(workbenchTabSet, {
    key: WORKBENCH_TABS_STORAGE_KEY,
    store: createWorkspaceStorageStore<WorkbenchTabSetState>(),
  })

  appearance.applyDocumentAppearance()

  return {
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    workbenchTabSet,
  } satisfies RestoredAppModels
}

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
} satisfies PersistStore<ShortcutRegistryState>

/** Reads the persisted appearance state before the first render and applies it to the document. */
export function getInitialAppearanceState() {
  const appearance = new Appearance(createDefaultAppearanceState())
  restoreSync(appearance, {
    key: APPEARANCE_STORAGE_KEY,
    store: createWorkspaceStorageStore<AppearanceState>(),
  })
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
    const persistenceHandles = [
      persist(appModels.appearance, {
        key: APPEARANCE_STORAGE_KEY,
        store: createWorkspaceStorageStore<AppearanceState>(),
      }),
      persist(appModels.navigation, {
        key: NAVIGATION_STORAGE_KEY,
        store: createWorkspaceStorageStore<NavigationState>(),
      }),
      persist(appModels.projectContext, {
        key: PROJECT_CONTEXT_STORAGE_KEY,
        store: createWorkspaceStorageStore<ProjectContextState>(),
      }),
      persist(appModels.projectRegistry, {
        key: PROJECT_REGISTRY_STORAGE_KEY,
        store: createWorkspaceStorageStore<ProjectRegistryState>(),
      }),
      persist(appModels.workbenchTabSet, {
        key: WORKBENCH_TABS_STORAGE_KEY,
        store: createWorkspaceStorageStore<WorkbenchTabSetState>(),
      }),
    ]
    const shortcutPersistence = hydrate(shortcutRegistry, {
      key: SHORTCUT_REGISTRY_STORAGE_KEY,
      store: shortcutRegistryStore,
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

    appModels.projectContext.syncProjects(
      appModels.projectRegistry.projectList.map((project) => project.path),
    )

    return () => {
      for (const handle of persistenceHandles) {
        void handle.stop()
      }

      void shortcutPersistence.stop()
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
