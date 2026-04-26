/** Allows browser entrypoints to import built CSS assets. */
declare module "*.css"

/** Allows importing SVG assets as raw markup strings. */
declare module "*.svg?raw" {
  const content: string
  export default content
}

/** Gives non-TSRX modules typed access to app context hooks until tsgo resolves `.tsrx` modules. */
declare module "~/app-state-context.tsrx" {
  import type { Protected } from "preact-sigma"
  import type { Appearance, AppearanceState } from "./appearance/appearance.ts"
  import type { Navigation } from "./navigation.ts"
  import type { ProjectContext } from "./projects/project-context.ts"
  import type { ProjectRegistry } from "./projects/project-registry.ts"
  import type { ShortcutRegistry } from "./shortcuts/shortcut-registry.ts"
  import type { WorkbenchTabSet } from "./workbench-tab-set.ts"

  export function AppStateProvider(props: {
    children: preact.ComponentChildren
    initialAppearanceState: AppearanceState
  }): preact.JSX.Element
  export function useAppearance(): Protected<Appearance>
  export function useNavigation(): Protected<Navigation>
  export function useProjectContext(): Protected<ProjectContext>
  export function useProjectRegistry(): Protected<ProjectRegistry>
  export function useShortcutRegistry(): ShortcutRegistry
  export function useWorkbenchTabSet(): Protected<WorkbenchTabSet>
}

/** Electrobun re-exports Three.js without types. This fixes type-checking errors. */
declare module "three"
