import { createContext } from "preact"
import { useContext } from "preact/hooks"

import { type PersistentAppModels, usePersistentAppModels } from "./app-state-persistence.ts"
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
  Appearance: createRequiredContext<PersistentAppModels["appearance"]>("appearanceContext"),
  Navigation: createRequiredContext<PersistentAppModels["navigation"]>("navigationContext"),
  ProjectContext: createRequiredContext<PersistentAppModels["projectContext"]>(
    "projectContextContext",
  ),
  ProjectRegistry: createRequiredContext<PersistentAppModels["projectRegistry"]>(
    "projectRegistryContext",
  ),
  ShortcutRegistry: createRequiredContext<PersistentAppModels["shortcutRegistry"]>(
    "shortcutRegistryContext",
  ),
  WorkbenchTabSet: createRequiredContext<PersistentAppModels["workbenchTabSet"]>(
    "workbenchTabSetContext",
  ),
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
  initialAppearanceState: AppearanceState;
}) {
  const appModels = usePersistentAppModels(props.initialAppearanceState)

  <context.Appearance value={appModels.appearance}>
    <context.Navigation value={appModels.navigation}>
      <context.ProjectContext value={appModels.projectContext}>
        <context.ProjectRegistry value={appModels.projectRegistry}>
          <context.ShortcutRegistry value={appModels.shortcutRegistry}>
            <context.WorkbenchTabSet value={appModels.workbenchTabSet}>
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
