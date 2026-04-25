import { createContext } from "preact"
import { type Protected, useSigma } from "preact-sigma"
import { useContext, useEffect, useRef } from "preact/hooks"

import {
  hydrateShortcutRegistryState,
  persistAppearanceState,
  persistNavigationState,
  persistProjectContextState,
  persistProjectRegistryState,
  persistWorkbenchTabSetState,
  restoreAppearanceState,
  restoreNavigationState,
  restoreProjectContextState,
  restoreProjectRegistryState,
  restoreWorkbenchTabSetState,
} from "./app-state-persistence.ts"
import { Appearance } from "./appearance/appearance.ts"
import { getSystemThemeMediaQuery, type AppearanceSnapshot } from "./appearance/theme.ts"
import { Navigation } from "./navigation.ts"
import { ProjectContext } from "./projects/project-context.ts"
import { ProjectRegistry } from "./projects/project-registry.ts"
import { shortcutRegistry, type ShortcutRegistry } from "./shortcuts/shortcut-registry.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"

const appearanceContext = createContext<Protected<Appearance> | null>(null)
const navigationContext = createContext<Protected<Navigation> | null>(null)
const projectContextContext = createContext<Protected<ProjectContext> | null>(null)
const projectRegistryContext = createContext<Protected<ProjectRegistry> | null>(null)
const shortcutRegistryContext = createContext<ShortcutRegistry | null>(null)
const workbenchTabSetContext = createContext<Protected<WorkbenchTabSet> | null>(null)

function requireContext<Value>(value: Value | null, name: string) {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

function requireRef<Value>(value: Value | null, name: string) {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
  initialAppearanceSnapshot: AppearanceSnapshot;
}) {
  const appearanceRef = useRef<Appearance | null>(null)
  const navigationRef = useRef<Navigation | null>(null)
  const projectContextRef = useRef<ProjectContext | null>(null)
  const projectRegistryRef = useRef<ProjectRegistry | null>(null)
  const workbenchTabSetRef = useRef<WorkbenchTabSet | null>(null)

  const appearance = useSigma(() => {
    const instance = new Appearance(props.initialAppearanceSnapshot)
    restoreAppearanceState(instance)
    appearanceRef.current = instance
    return instance
  }, {
    deps: [props.initialAppearanceSnapshot],
  })
  const navigation = useSigma(() => {
    const instance = new Navigation()
    restoreNavigationState(instance)
    navigationRef.current = instance
    return instance
  })
  const projectContext = useSigma(() => {
    const instance = new ProjectContext()
    restoreProjectContextState(instance)
    projectContextRef.current = instance
    return instance
  })
  const projectRegistry = useSigma(() => {
    const instance = new ProjectRegistry()
    restoreProjectRegistryState(instance)
    projectRegistryRef.current = instance
    return instance
  })
  const workbenchTabSet = useSigma(() => {
    const instance = new WorkbenchTabSet()
    restoreWorkbenchTabSetState(instance)
    workbenchTabSetRef.current = instance
    return instance
  })

  useEffect(() => {
    const appearanceInstance = requireRef(appearanceRef.current, "appearance")
    const navigationInstance = requireRef(navigationRef.current, "navigation")
    const projectContextInstance = requireRef(projectContextRef.current, "projectContext")
    const projectRegistryInstance = requireRef(projectRegistryRef.current, "projectRegistry")
    const workbenchTabSetInstance = requireRef(workbenchTabSetRef.current, "workbenchTabSet")
    const persistenceHandles = [
      persistAppearanceState(appearanceInstance),
      persistNavigationState(navigationInstance),
      persistProjectContextState(projectContextInstance),
      persistProjectRegistryState(projectRegistryInstance),
      persistWorkbenchTabSetState(workbenchTabSetInstance),
    ]
    const shortcutPersistence = hydrateShortcutRegistryState(shortcutRegistry)
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    projectContext.syncProjects(projectRegistry.projectList.map((project) => project.path))

    const stopPersistence = () => {
      for (const handle of persistenceHandles) {
        void handle.stop()
      }
      void shortcutPersistence.stop()
    }

    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return () => {
        stopPersistence()
        cleanupShortcutRegistry()
      }
    }

    const mediaQuery = window.matchMedia(getSystemThemeMediaQuery())
    const syncSystemTheme = () => {
      appearance.syncSystemTheme(mediaQuery.matches ? "dark" : "light")
    }

    syncSystemTheme()
    mediaQuery.addEventListener("change", syncSystemTheme)

    return () => {
      mediaQuery.removeEventListener("change", syncSystemTheme)
      stopPersistence()
      cleanupShortcutRegistry()
    }
  }, [
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    workbenchTabSet,
  ])

  <appearanceContext.Provider value={appearance}>
    <navigationContext.Provider value={navigation}>
      <projectContextContext.Provider value={projectContext}>
        <projectRegistryContext.Provider value={projectRegistry}>
          <shortcutRegistryContext.Provider value={shortcutRegistry}>
            <workbenchTabSetContext.Provider value={workbenchTabSet}>
              {props.children}
            </workbenchTabSetContext.Provider>
          </shortcutRegistryContext.Provider>
        </projectRegistryContext.Provider>
      </projectContextContext.Provider>
    </navigationContext.Provider>
  </appearanceContext.Provider>
}

export function useAppearance() {
  return requireContext(useContext(appearanceContext), "appearanceContext")
}

export function useNavigation() {
  return requireContext(useContext(navigationContext), "navigationContext")
}

export function useProjectContext() {
  return requireContext(useContext(projectContextContext), "projectContextContext")
}

export function useProjectRegistry() {
  return requireContext(useContext(projectRegistryContext), "projectRegistryContext")
}

export function useShortcutRegistry() {
  return requireContext(useContext(shortcutRegistryContext), "shortcutRegistryContext")
}

export function useWorkbenchTabSet() {
  return requireContext(useContext(workbenchTabSetContext), "workbenchTabSetContext")
}
