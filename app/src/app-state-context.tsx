import { createContext } from "preact"
import { useSigma } from "preact-sigma"
import { useContext, useEffect } from "preact/hooks"

import { ProjectRegistry } from "~/projects/project-registry.ts"
import { Appearance } from "./appearance/appearance.ts"
import { getSystemThemeMediaQuery, type AppearanceSnapshot } from "./appearance/theme.ts"
import { Navigation } from "./navigation.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"

const appearanceContext = createContext<Appearance | null>(null)
const navigationContext = createContext<Navigation | null>(null)
const projectRegistryContext = createContext<ProjectRegistry | null>(null)
const workbenchTabSetContext = createContext<WorkbenchTabSet | null>(null)

function requireContext<Value>(value: Value | null, name: string): Value {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

export function AppStateProvider(props: {
  children: preact.ComponentChildren
  initialAppearanceSnapshot: AppearanceSnapshot
}) {
  const appearance = useSigma(
    () => new Appearance(props.initialAppearanceSnapshot),
    [props.initialAppearanceSnapshot],
  )
  const navigation = useSigma(() => new Navigation())
  const projectRegistry = useSigma(() => new ProjectRegistry())
  const workbenchTabSet = useSigma(() => new WorkbenchTabSet())

  useEffect(() => {
    navigation.hydrateNavigation()
    workbenchTabSet.hydrateTabsFromStore()
    projectRegistry.loadProjects()

    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return
    }

    const mediaQuery = window.matchMedia(getSystemThemeMediaQuery())
    const syncSystemTheme = () => {
      appearance.syncSystemTheme(mediaQuery.matches ? "dark" : "light")
    }

    syncSystemTheme()
    mediaQuery.addEventListener("change", syncSystemTheme)

    return () => {
      mediaQuery.removeEventListener("change", syncSystemTheme)
    }
  }, [appearance, navigation, projectRegistry, workbenchTabSet])

  return (
    <appearanceContext.Provider value={appearance}>
      <navigationContext.Provider value={navigation}>
        <projectRegistryContext.Provider value={projectRegistry}>
          <workbenchTabSetContext.Provider value={workbenchTabSet}>
            {props.children}
          </workbenchTabSetContext.Provider>
        </projectRegistryContext.Provider>
      </navigationContext.Provider>
    </appearanceContext.Provider>
  )
}

export function useAppearance() {
  return requireContext(useContext(appearanceContext), "appearanceContext")
}

export function useNavigation() {
  return requireContext(useContext(navigationContext), "navigationContext")
}

export function useProjectRegistry() {
  return requireContext(useContext(projectRegistryContext), "projectRegistryContext")
}

export function useWorkbenchTabSet() {
  return requireContext(useContext(workbenchTabSetContext), "workbenchTabSetContext")
}
