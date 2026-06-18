import { createContext } from "preact"
import { useContext } from "preact/hooks"

import { type AppState, useAppState } from "./app-state-persistence.ts"

type RequiredContext<Value> = preact.Context<Value | null> & {
    use(): Value;
  }

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

function defineAppStateContext<Key extends Extract<keyof AppState, string>>(name: Key): RequiredContext<AppState[Key]> {
  type Value = AppState[Key]

  const existingContext = requiredContextStore[name] as RequiredContext<Value> | undefined

  if (existingContext) {
    return existingContext
  }

  const nextContext = createContext<Value | null>(null) as RequiredContext<Value>

  nextContext.use = () => {
    const value = useContext(nextContext)

    if (!value) {
      throw new Error(`Context "${name}" is missing.`)
    }

    return value
  }

  requiredContextStore[name] = nextContext as RequiredContext<unknown>
  return nextContext
}

const context = {
  Appearance: defineAppStateContext("appearance"),
  MainTab: defineAppStateContext("mainTab"),
  ProjectContext: defineAppStateContext("projectContext"),
  ProjectRegistry: defineAppStateContext("projectRegistry"),
  ShortcutRegistry: defineAppStateContext("shortcutRegistry"),
  WorkbenchTabCache: defineAppStateContext("workbenchTabCache"),
  WorkbenchTabSet: defineAppStateContext("workbenchTabSet"),
}

export function AppStateProvider(props: {
  children: preact.ComponentChildren;
}) @{
  const appState = useAppState()

  <context.Appearance value={appState.appearance}>
    <context.MainTab value={appState.mainTab}>
      <context.ProjectContext value={appState.projectContext}>
        <context.ProjectRegistry value={appState.projectRegistry}>
          <context.ShortcutRegistry value={appState.shortcutRegistry}>
            <context.WorkbenchTabCache value={appState.workbenchTabCache}>
              <context.WorkbenchTabSet value={appState.workbenchTabSet}>
                @if (appState.hydrationStatus === "ready") {
                  <>
                    {props.children}
                  </>
                }
              </context.WorkbenchTabSet>
            </context.WorkbenchTabCache>
          </context.ShortcutRegistry>
        </context.ProjectRegistry>
      </context.ProjectContext>
    </context.MainTab>
  </context.Appearance>
}

export const useAppearance = context.Appearance.use
export const useMainTab = context.MainTab.use
export const useProjectContext = context.ProjectContext.use
export const useProjectRegistry = context.ProjectRegistry.use
export const useShortcutRegistry = context.ShortcutRegistry.use
export const useWorkbenchTabCache = context.WorkbenchTabCache.use
export const useWorkbenchTabSet = context.WorkbenchTabSet.use
