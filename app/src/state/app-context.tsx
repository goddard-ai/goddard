import type { ComponentChildren } from "preact"
import { createContext } from "preact"
import { useContext, useEffect } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { Navigation } from "./navigation-state"
import { ProjectRegistry } from "./project-registry"
import { WorkbenchTabSet } from "./workbench-tabs-state"

const navigationContext = createContext<Navigation | null>(null)
const projectRegistryContext = createContext<ProjectRegistry | null>(null)
const workbenchTabSetContext = createContext<WorkbenchTabSet | null>(null)

function requireContext<Value>(value: Value | null, name: string): Value {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

/** Boots the app's shared sigma instances and exposes them through Preact Context. */
export function AppStateProvider(props: { children: ComponentChildren }) {
  const navigation = useSigma(() => new Navigation())
  const projectRegistry = useSigma(() => new ProjectRegistry())
  const workbenchTabSet = useSigma(() => new WorkbenchTabSet())

  useEffect(() => {
    navigation.hydrateNavigation()
    workbenchTabSet.hydrateTabsFromStore()
    projectRegistry.loadProjects()
  }, [navigation, projectRegistry, workbenchTabSet])

  useEffect(() => {
    navigation.setBadgeCount("projects", projectRegistry.projectList.length)
  }, [navigation, projectRegistry.projectList.length])

  return (
    <navigationContext.Provider value={navigation}>
      <projectRegistryContext.Provider value={projectRegistry}>
        <workbenchTabSetContext.Provider value={workbenchTabSet}>
          {props.children}
        </workbenchTabSetContext.Provider>
      </projectRegistryContext.Provider>
    </navigationContext.Provider>
  )
}

/** Returns the shared primary navigation sigma instance. */
export function useNavigation(): Navigation {
  return requireContext(useContext(navigationContext), "navigationContext")
}

/** Returns the shared project registry sigma instance. */
export function useProjectRegistry(): ProjectRegistry {
  return requireContext(useContext(projectRegistryContext), "projectRegistryContext")
}

/** Returns the shared workbench tab sigma instance. */
export function useWorkbenchTabSet(): WorkbenchTabSet {
  return requireContext(useContext(workbenchTabSetContext), "workbenchTabSetContext")
}
