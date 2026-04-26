import { createContext } from "preact"
import { type Protected, useSigma } from "preact-sigma"
import { useContext, useEffect, useRef } from "preact/hooks"

import {
  hydrateShortcutRegistryState,
  persistAppearanceState,
  persistNavigationState,
  persistProjectContextState,
  persistProjectRegistryState,
  persistWorkbenchTabSetState,
  restoreAppearanceState,
  restoreNavigationState,
  restoreProjectContextState,
  restoreProjectRegistryState,
  restoreWorkbenchTabSetState,
} from "./app-state-persistence.ts"
import { Appearance, type AppearanceState } from "./appearance/appearance.ts"
import { getSystemThemeMediaQuery } from "./appearance/theme.ts"
import { Navigation } from "./navigation.ts"
import { ProjectContext } from "./projects/project-context.ts"
import { ProjectRegistry } from "./projects/project-registry.ts"
import { shortcutRegistry, type ShortcutRegistry } from "./shortcuts/shortcut-registry.ts"
import { WorkbenchTabSet } from "./workbench-tab-set.ts"

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
  Appearance: createRequiredContext<Protected<Appearance>>("appearanceContext"),
  Navigation: createRequiredContext<Protected<Navigation>>("navigationContext"),
  ProjectContext: createRequiredContext<Protected<ProjectContext>>("projectContextContext"),
  ProjectRegistry: createRequiredContext<Protected<ProjectRegistry>>("projectRegistryContext"),
  ShortcutRegistry: createRequiredContext<ShortcutRegistry>("shortcutRegistryContext"),
  WorkbenchTabSet: createRequiredContext<Protected<WorkbenchTabSet>>("workbenchTabSetContext"),
}

function requireRef<Value>(value: Value | null, name: string) {
  if (!value) {
    throw new Error(`${name} is missing.`)
  }

  return value
}

export component AppStateProvider(props: {
  children: preact.ComponentChildren;
  initialAppearanceState: AppearanceState;
}) {
  const appearanceRef = useRef<Appearance | null>(null)
  const navigationRef = useRef<Navigation | null>(null)
  const projectContextRef = useRef<ProjectContext | null>(null)
  const projectRegistryRef = useRef<ProjectRegistry | null>(null)
  const workbenchTabSetRef = useRef<WorkbenchTabSet | null>(null)

  const appearance = useSigma(() => {
    const instance = new Appearance(props.initialAppearanceState)
    restoreAppearanceState(instance)
    appearanceRef.current = instance
    return instance
  }, {
    deps: [props.initialAppearanceState],
  })
  const navigation = useSigma(() => {
    const instance = new Navigation()
    restoreNavigationState(instance)
    navigationRef.current = instance
    return instance
  })
  const projectContext = useSigma(() => {
    const instance = new ProjectContext()
    restoreProjectContextState(instance)
    projectContextRef.current = instance
    return instance
  })
  const projectRegistry = useSigma(() => {
    const instance = new ProjectRegistry()
    restoreProjectRegistryState(instance)
    projectRegistryRef.current = instance
    return instance
  })
  const workbenchTabSet = useSigma(() => {
    const instance = new WorkbenchTabSet()
    restoreWorkbenchTabSetState(instance)
    workbenchTabSetRef.current = instance
    return instance
  })

  useEffect(() => {
    const appearance = requireRef(appearanceRef.current, "appearance")
    const navigation = requireRef(navigationRef.current, "navigation")
    const projectContext = requireRef(projectContextRef.current, "projectContext")
    const projectRegistry = requireRef(projectRegistryRef.current, "projectRegistry")
    const workbenchTabSet = requireRef(workbenchTabSetRef.current, "workbenchTabSet")
    const persistenceHandles = [
      persistAppearanceState(appearance),
      persistNavigationState(navigation),
      persistProjectContextState(projectContext),
      persistProjectRegistryState(projectRegistry),
      persistWorkbenchTabSetState(workbenchTabSet),
    ]
    const shortcutPersistence = hydrateShortcutRegistryState(shortcutRegistry)
    const cleanupShortcutRegistry = shortcutRegistry.setup()

    projectContext.syncProjects(projectRegistry.projectList.map((project) => project.path))

    const stopPersistence = () => {
      for (const handle of persistenceHandles) {
        void handle.stop()
      }
      void shortcutPersistence.stop()
    }

    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return () => {
        stopPersistence()
        cleanupShortcutRegistry()
      }
    }

    const mediaQuery = window.matchMedia(getSystemThemeMediaQuery())
    const syncSystemTheme = () => {
      appearance.syncSystemTheme(mediaQuery.matches ? "dark" : "light")
    }

    syncSystemTheme()
    mediaQuery.addEventListener("change", syncSystemTheme)

    return () => {
      mediaQuery.removeEventListener("change", syncSystemTheme)
      stopPersistence()
      cleanupShortcutRegistry()
    }
  }, [
    appearance,
    navigation,
    projectContext,
    projectRegistry,
    workbenchTabSet,
  ])

  <context.Appearance value={appearance}>
    <context.Navigation value={navigation}>
      <context.ProjectContext value={projectContext}>
        <context.ProjectRegistry value={projectRegistry}>
          <context.ShortcutRegistry value={shortcutRegistry}>
            <context.WorkbenchTabSet value={workbenchTabSet}>
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
