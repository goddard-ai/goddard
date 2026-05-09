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

function createRequiredContext<Value>(name: string): RequiredContext<Value> {
  const nextContext = createContext<Value | null>(null) as RequiredContext<Value>

  nextContext.use = () => {
    return requireContext(useContext(nextContext), name)
  }

  return nextContext
}

const context = {
  Appearance: createRequiredContext<AppState["appearance"]>("appearanceContext"),
  Inbox: createRequiredContext<AppState["inbox"]>("inboxContext"),
  Navigation: createRequiredContext<AppState["navigation"]>("navigationContext"),
  ProjectContext: createRequiredContext<AppState["projectContext"]>("projectContextContext"),
  ProjectRegistry: createRequiredContext<AppState["projectRegistry"]>("projectRegistryContext"),
  ShortcutRegistry: createRequiredContext<AppState["shortcutRegistry"]>("shortcutRegistryContext"),
  WorkbenchTabSet: createRequiredContext<AppState["workbenchTabSet"]>("workbenchTabSetContext"),
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
}) {
  const appState = useAppState()

  <context.Appearance value={appState.appearance}>
    <context.Inbox value={appState.inbox}>
      <context.Navigation value={appState.navigation}>
        <context.ProjectContext value={appState.projectContext}>
          <context.ProjectRegistry value={appState.projectRegistry}>
            <context.ShortcutRegistry value={appState.shortcutRegistry}>
              <context.WorkbenchTabSet value={appState.workbenchTabSet}>
                {props.children}
              </context.WorkbenchTabSet>
            </context.ShortcutRegistry>
          </context.ProjectRegistry>
        </context.ProjectContext>
      </context.Navigation>
    </context.Inbox>
  </context.Appearance>
}

export const useAppearance = context.Appearance.use
export const useInbox = context.Inbox.use
export const useNavigation = context.Navigation.use
export const useProjectContext = context.ProjectContext.use
export const useProjectRegistry = context.ProjectRegistry.use
export const useShortcutRegistry = context.ShortcutRegistry.use
export const useWorkbenchTabSet = context.WorkbenchTabSet.use
