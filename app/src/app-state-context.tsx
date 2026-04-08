import type { ComponentChildren } from "preact"
import { createContext } from "preact"
import { useContext, useEffect } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { Navigation } from "./navigation.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"
import { ProjectRegistry } from "~/projects/project-registry.ts"

const navigationContext = createContext<Navigation | null>(null)
const projectRegistryContext = createContext<ProjectRegistry | null>(null)
const workbenchTabSetContext = createContext<WorkbenchTabSet | null>(null)

function requireContext<Value>(value: Value | null, name: string): Value {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

export function AppStateProvider(props: { children: ComponentChildren }) {
  const navigation = useSigma(() => new Navigation())
  const projectRegistry = useSigma(() => new ProjectRegistry())
  const workbenchTabSet = useSigma(() => new WorkbenchTabSet())

  useEffect(() => {
    navigation.hydrateNavigation()
    workbenchTabSet.hydrateTabsFromStore()
    projectRegistry.loadProjects()
  }, [navigation, projectRegistry, workbenchTabSet])

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

export function useNavigation() {
  return requireContext(useContext(navigationContext), "navigationContext")
}

export function useProjectRegistry() {
  return requireContext(useContext(projectRegistryContext), "projectRegistryContext")
}

export function useWorkbenchTabSet() {
  return requireContext(useContext(workbenchTabSetContext), "workbenchTabSetContext")
}
