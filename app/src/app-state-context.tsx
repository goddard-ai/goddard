import type { ComponentChildren } from "preact"
import { createContext } from "preact"
import { useContext, useEffect } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { Navigation } from "./navigation.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"
import { ProjectRegistry } from "~/projects/project-registry.ts"
import { SessionLaunch } from "~/sessions/session-launch.ts"

const navigationContext = createContext<Navigation | null>(null)
const projectRegistryContext = createContext<ProjectRegistry | null>(null)
const sessionLaunchContext = createContext<SessionLaunch | null>(null)
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
  const sessionLaunch = useSigma(() => new SessionLaunch())
  const workbenchTabSet = useSigma(() => new WorkbenchTabSet())

  useEffect(() => {
    navigation.hydrateNavigation()
    workbenchTabSet.hydrateTabsFromStore()
    projectRegistry.loadProjects()
  }, [navigation, projectRegistry, workbenchTabSet])

  return (
    <navigationContext.Provider value={navigation}>
      <projectRegistryContext.Provider value={projectRegistry}>
        <sessionLaunchContext.Provider value={sessionLaunch}>
          <workbenchTabSetContext.Provider value={workbenchTabSet}>
            {props.children}
          </workbenchTabSetContext.Provider>
        </sessionLaunchContext.Provider>
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

export function useSessionLaunch() {
  return requireContext(useContext(sessionLaunchContext), "sessionLaunchContext")
}

export function useWorkbenchTabSet() {
  return requireContext(useContext(workbenchTabSetContext), "workbenchTabSetContext")
}
