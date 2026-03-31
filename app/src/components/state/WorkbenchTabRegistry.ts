import type { ComponentProps } from "preact"
import { lazy } from "preact/compat"
import type { ShellIconName } from "../../support/shell-icons"

/** Shared fields carried by every closable workbench tab. */
export type WorkbenchTabBase = {
  id: string
  title: string
  icon: ShellIconName
  dirty: boolean
}

/** Props received by one lazily rendered workbench tab component. */
export type WorkbenchTabComponentProps<TKind extends string, TPayload> = {
  tab: WorkbenchTabBase & {
    kind: TKind
    payload: TPayload
  }
}

/** Runtime registry for every closable workbench tab component. */
export const workbenchTabComponents = {
  project: lazy(() => import("../Projects/ProjectPage")),
  projects: lazy(() => import("../Projects/ProjectsTab")),
} as const

/** The supported closable workbench tab kinds available in the shell. */
export type WorkbenchTabKind = keyof typeof workbenchTabComponents

/** Payload inferred from one registered closable workbench tab component. */
type WorkbenchTabPayload<TKind extends WorkbenchTabKind> = ComponentProps<
  (typeof workbenchTabComponents)[TKind]
>["tab"]["payload"]

/** One closable workbench tab tracked by the shell. */
export type WorkbenchTab<TKind extends WorkbenchTabKind = WorkbenchTabKind> = WorkbenchTabBase & {
  kind: TKind
  payload: WorkbenchTabPayload<TKind>
}

/** Returns the lazily loaded component registered for one closable workbench tab kind. */
export function getWorkbenchTabComponent<TKind extends WorkbenchTabKind>(
  kind: TKind,
): (typeof workbenchTabComponents)[TKind] {
  return workbenchTabComponents[kind]
}

/** Returns whether one runtime string matches a registered closable workbench tab kind. */
export function isWorkbenchTabKind(kind: string): kind is WorkbenchTabKind {
  return kind in workbenchTabComponents
}
