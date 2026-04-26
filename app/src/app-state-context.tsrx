import { createContext } from "preact"
import { useContext } from "preact/hooks"

import { type PersistentAppState, usePersistentAppState } from "./app-state-persistence.ts"
import type { AppearanceState } from "./appearance/appearance.ts"

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
  Appearance: createRequiredContext<PersistentAppState["appearance"]>("appearanceContext"),
  Navigation: createRequiredContext<PersistentAppState["navigation"]>("navigationContext"),
  ProjectContext: createRequiredContext<PersistentAppState["projectContext"]>(
    "projectContextContext",
  ),
  ProjectRegistry: createRequiredContext<PersistentAppState["projectRegistry"]>(
    "projectRegistryContext",
  ),
  ShortcutRegistry: createRequiredContext<PersistentAppState["shortcutRegistry"]>(
    "shortcutRegistryContext",
  ),
  WorkbenchTabSet: createRequiredContext<PersistentAppState["workbenchTabSet"]>(
    "workbenchTabSetContext",
  ),
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
  initialAppearanceState: AppearanceState;
}) {
  const appState = usePersistentAppState(props.initialAppearanceState)

  <context.Appearance value={appState.appearance}>
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
  </context.Appearance>
}

export const useAppearance = context.Appearance.use
export const useNavigation = context.Navigation.use
export const useProjectContext = context.ProjectContext.use
export const useProjectRegistry = context.ProjectRegistry.use
export const useShortcutRegistry = context.ShortcutRegistry.use
export const useWorkbenchTabSet = context.WorkbenchTabSet.use
