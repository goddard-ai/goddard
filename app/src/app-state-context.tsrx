import { createContext } from "preact"
import { type Protected, useSigma } from "preact-sigma"
import { useContext, useEffect } from "preact/hooks"

import { ProjectContext } from "~/projects/project-context.ts"
import { ProjectRegistry } from "~/projects/project-registry.ts"
import { shortcutRegistry, type ShortcutRegistry } from "~/shortcuts/shortcut-registry.ts"
import { Appearance } from "./appearance/appearance.ts"
import { getSystemThemeMediaQuery, type AppearanceSnapshot } from "./appearance/theme.ts"
import { Navigation } from "./navigation.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"

const appearanceContext = createContext<Protected<Appearance> | null>(null)
const navigationContext = createContext<Protected<Navigation> | null>(null)
const projectContextContext = createContext<Protected<ProjectContext> | null>(null)
const projectRegistryContext = createContext<Protected<ProjectRegistry> | null>(null)
const shortcutRegistryContext = createContext<ShortcutRegistry | null>(null)
const workbenchTabSetContext = createContext<Protected<WorkbenchTabSet> | null>(null)

function requireContext<Value>(value: Value | null, name: string): Value {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
  initialAppearanceSnapshot: AppearanceSnapshot;
}) {
  const appearance = useSigma(() => new Appearance(props.initialAppearanceSnapshot), {
    deps: [props.initialAppearanceSnapshot],
  })
  const navigation = useSigma(() => new Navigation())
  const projectContext = useSigma(() => new ProjectContext())
  const projectRegistry = useSigma(() => new ProjectRegistry())
  const workbenchTabSet = useSigma(() => new WorkbenchTabSet())

  useEffect(() => {
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    navigation.hydrateNavigation()
    workbenchTabSet.hydrateTabsFromStore()
    projectRegistry.loadProjects()
    projectContext.hydrate(projectRegistry.projectList.map((project) => project.path))
    void shortcutRegistry.hydrateKeymap()

    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return cleanupShortcutRegistry
    }

    const mediaQuery = window.matchMedia(getSystemThemeMediaQuery())
    const syncSystemTheme = () => {
      appearance.syncSystemTheme(mediaQuery.matches ? "dark" : "light")
    }

    syncSystemTheme()
    mediaQuery.addEventListener("change", syncSystemTheme)

    return () => {
      mediaQuery.removeEventListener("change", syncSystemTheme)
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
