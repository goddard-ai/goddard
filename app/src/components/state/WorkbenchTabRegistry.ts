import type { FunctionComponent } from "preact"
import { lazy } from "preact/compat"
import type { ShellIconName } from "../../support/shell-icons"

/** Payload props expected by each closable workbench tab component. */
type WorkbenchTabPayloadByKind = {
  project: {
    projectPath: string
  }
  projects: Record<string, never>
}

/** One lazily loaded closable tab component keyed by its payload props. */
type WorkbenchTabComponentByKind = {
  [TKind in keyof WorkbenchTabPayloadByKind]: FunctionComponent<WorkbenchTabPayloadByKind[TKind]>
}

/** One loosely typed lazily rendered closable tab component. */
type LooseWorkbenchTabComponent = FunctionComponent<any>

/** Runtime registry for every closable workbench tab component. */
export const workbenchTabComponents: WorkbenchTabComponentByKind = {
  project: lazy(() => import("../Projects/ProjectPage")),
  projects: lazy(() => import("../Projects/ProjectsPage")),
}

/** The supported closable workbench tab kinds available in the shell. */
export type WorkbenchDetailTabKind = keyof typeof workbenchTabComponents

/** Payload inferred from one registered closable workbench tab component. */
type WorkbenchTabPayload<TKind extends WorkbenchDetailTabKind> = WorkbenchTabPayloadByKind[TKind]

/** One workbench tab tracked by the shell, including the always-present main tab. */
type WorkbenchTabByKind = {
  main: {
    id: "main"
    kind: "main"
    title: string
    icon: ShellIconName
    dirty?: undefined
    payload?: undefined
  }
} & {
  [TKind in WorkbenchDetailTabKind]: {
    id: string
    kind: TKind
    title: string
    icon: ShellIconName
    dirty: boolean
    payload: WorkbenchTabPayload<TKind>
  }
}

/** The supported workbench tab kinds available in the shell. */
export type WorkbenchTabKind = keyof WorkbenchTabByKind

/** One workbench tab tracked by the shell. */
export type WorkbenchTab<TKind extends WorkbenchTabKind = WorkbenchTabKind> =
  WorkbenchTabByKind[TKind]

/** Returns the lazily loaded component registered for one closable workbench tab kind. */
export function getWorkbenchTabComponent(kind: WorkbenchDetailTabKind): LooseWorkbenchTabComponent {
  return workbenchTabComponents[kind]
}

/** Returns whether one runtime string matches a registered closable workbench tab kind. */
export function isWorkbenchDetailTabKind(kind: string): kind is WorkbenchDetailTabKind {
  return kind in workbenchTabComponents
}
