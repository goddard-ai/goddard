import type { ComponentChildren } from "preact"
import { createContext } from "preact"
import { useContext, useEffect } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { ProjectRegistry } from "~/projects/project-registry.ts"
import { SessionChat } from "~/session-chat/chat.ts"
import { SessionIndex } from "~/sessions/session-index.ts"
import { SessionLaunch } from "~/sessions/session-launch.ts"
import { desktopSessionService } from "~/sessions/session-service.ts"
import { Navigation } from "./navigation.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"

const navigationContext = createContext<Navigation | null>(null)
const projectRegistryContext = createContext<ProjectRegistry | null>(null)
const sessionChatContext = createContext<SessionChat | null>(null)
const sessionIndexContext = createContext<SessionIndex | null>(null)
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
  const sessionChat = useSigma(() => new SessionChat())
  const sessionIndex = useSigma(() => new SessionIndex())
  const sessionLaunch = useSigma(() => new SessionLaunch())
  const workbenchTabSet = useSigma(() => new WorkbenchTabSet())

  useEffect(() => {
    navigation.hydrateNavigation()
    workbenchTabSet.hydrateTabsFromStore()
    projectRegistry.loadProjects()
    void sessionIndex.refreshSessions(desktopSessionService)
  }, [navigation, projectRegistry, sessionIndex, workbenchTabSet])

  return (
    <navigationContext.Provider value={navigation}>
      <projectRegistryContext.Provider value={projectRegistry}>
        <sessionIndexContext.Provider value={sessionIndex}>
          <sessionChatContext.Provider value={sessionChat}>
            <sessionLaunchContext.Provider value={sessionLaunch}>
              <workbenchTabSetContext.Provider value={workbenchTabSet}>
                {props.children}
              </workbenchTabSetContext.Provider>
            </sessionLaunchContext.Provider>
          </sessionChatContext.Provider>
        </sessionIndexContext.Provider>
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

export function useSessionIndex() {
  return requireContext(useContext(sessionIndexContext), "sessionIndexContext")
}

export function useSessionChat() {
  return requireContext(useContext(sessionChatContext), "sessionChatContext")
}

export function useSessionLaunch() {
  return requireContext(useContext(sessionLaunchContext), "sessionLaunchContext")
}

export function useWorkbenchTabSet() {
  return requireContext(useContext(workbenchTabSetContext), "workbenchTabSetContext")
}
