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

type AppStateContextStore = {
  Appearance?: RequiredContext<AppState["appearance"]>;
  Inbox?: RequiredContext<AppState["inbox"]>;
  MainTab?: RequiredContext<AppState["mainTab"]>;
  ProjectContext?: RequiredContext<AppState["projectContext"]>;
  ProjectRegistry?: RequiredContext<AppState["projectRegistry"]>;
  ShortcutRegistry?: RequiredContext<AppState["shortcutRegistry"]>;
  WorkbenchTabCache?: RequiredContext<AppState["workbenchTabCache"]>;
  WorkbenchTabSet?: RequiredContext<AppState["workbenchTabSet"]>;
}

// Keep context identity stable when Vite re-evaluates this module during HMR.
const appStateContextStore = (globalThis as typeof globalThis & {
  __goddardAppStateContexts?: AppStateContextStore;
}).__goddardAppStateContexts ??= {}

function createRequiredContext<Value>(name: string): RequiredContext<Value> {
  const nextContext = createContext<Value | null>(null) as RequiredContext<Value>

  nextContext.use = () => {
    return requireContext(useContext(nextContext), name)
  }

  return nextContext
}

const context = {
  Appearance: appStateContextStore.Appearance ??= createRequiredContext<AppState["appearance"]>(
    "appearanceContext",
  ),
  Inbox: appStateContextStore.Inbox ??= createRequiredContext<AppState["inbox"]>("inboxContext"),
  MainTab: appStateContextStore.MainTab ??= createRequiredContext<AppState["mainTab"]>(
    "mainTabContext",
  ),
  ProjectContext: appStateContextStore.ProjectContext ??= createRequiredContext<
    AppState["projectContext"]
  >("projectContextContext"),
  ProjectRegistry: appStateContextStore.ProjectRegistry ??= createRequiredContext<
    AppState["projectRegistry"]
  >("projectRegistryContext"),
  ShortcutRegistry: appStateContextStore.ShortcutRegistry ??= createRequiredContext<
    AppState["shortcutRegistry"]
  >("shortcutRegistryContext"),
  WorkbenchTabCache: appStateContextStore.WorkbenchTabCache ??= createRequiredContext<
    AppState["workbenchTabCache"]
  >("workbenchTabCacheContext"),
  WorkbenchTabSet: appStateContextStore.WorkbenchTabSet ??= createRequiredContext<
    AppState["workbenchTabSet"]
  >("workbenchTabSetContext"),
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
