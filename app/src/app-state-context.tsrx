import { createContext } from "preact"
import { useContext } from "preact/hooks"

import { type AppState, useAppState } from "./app-state-persistence.ts"

function requireContext<Value>(value: Value | null, name: string) {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

type RequiredContext<Value> = preact.Context<Value | null> & {
    use(): Value;
  }

type AppStateContextName = `${Extract<keyof AppState, string>}Context`
type AppStateContextKey<Name extends AppStateContextName> =
  Name extends `${infer Key}Context` ? Key & keyof AppState : never
type RequiredContextStore = Record<string, RequiredContext<unknown>>

type AppStateContextHotData = {
  requiredContextStore?: RequiredContextStore;
}

// Keep context identity stable when Vite re-evaluates this module during HMR.
const appStateContextHotData = import.meta.hot?.data as AppStateContextHotData | undefined
const requiredContextStore = appStateContextHotData?.requiredContextStore ?? {}

if (appStateContextHotData) {
  appStateContextHotData.requiredContextStore = requiredContextStore
}

function defineAppStateContext<Name extends AppStateContextName>(name: Name): RequiredContext<AppState[AppStateContextKey<Name>]> {
  type Value = AppState[AppStateContextKey<Name>]

  const existingContext = requiredContextStore[name] as RequiredContext<Value> | undefined

  if (existingContext) {
    return existingContext
  }

  const nextContext = createContext<Value | null>(null) as RequiredContext<Value>

  nextContext.use = () => {
    return requireContext(useContext(nextContext), name)
  }

  requiredContextStore[name] = nextContext as RequiredContext<unknown>
  return nextContext
}

const context = {
  Appearance: defineAppStateContext("appearanceContext"),
  Inbox: defineAppStateContext("inboxContext"),
  MainTab: defineAppStateContext("mainTabContext"),
  ProjectContext: defineAppStateContext("projectContextContext"),
  ProjectRegistry: defineAppStateContext("projectRegistryContext"),
  ShortcutRegistry: defineAppStateContext("shortcutRegistryContext"),
  WorkbenchTabCache: defineAppStateContext("workbenchTabCacheContext"),
  WorkbenchTabSet: defineAppStateContext("workbenchTabSetContext"),
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
}) {
  const appState = useAppState()

  <context.Appearance value={appState.appearance}>
    <context.Inbox value={appState.inbox}>
      <context.MainTab value={appState.mainTab}>
        <context.ProjectContext value={appState.projectContext}>
          <context.ProjectRegistry value={appState.projectRegistry}>
            <context.ShortcutRegistry value={appState.shortcutRegistry}>
              <context.WorkbenchTabCache value={appState.workbenchTabCache}>
                <context.WorkbenchTabSet value={appState.workbenchTabSet}>
                  {props.children}
                </context.WorkbenchTabSet>
              </context.WorkbenchTabCache>
            </context.ShortcutRegistry>
          </context.ProjectRegistry>
        </context.ProjectContext>
      </context.MainTab>
    </context.Inbox>
  </context.Appearance>
}

export const useAppearance = context.Appearance.use
export const useInbox = context.Inbox.use
export const useMainTab = context.MainTab.use
export const useProjectContext = context.ProjectContext.use
export const useProjectRegistry = context.ProjectRegistry.use
export const useShortcutRegistry = context.ShortcutRegistry.use
export const useWorkbenchTabCache = context.WorkbenchTabCache.use
export const useWorkbenchTabSet = context.WorkbenchTabSet.use
