import type { ComponentProps, FunctionComponent } from "preact"
import { lazy } from "preact/compat"
import type { SvgIconName } from "./lib/good-icon.tsx"

/** One registered non-primary workbench tab definition. */
type WorkbenchTabDefinition = {
  component: FunctionComponent<any>
  icon: SvgIconName
}

/** One loosely typed lazily rendered non-primary tab component. */
type LooseWorkbenchTabComponent = FunctionComponent<any>

/** Placeholder workbench surface used until a real implementation exists. */
function PlaceholderWorkbenchTab() {
  return null
}

/** Runtime registry for every non-primary workbench tab component. */
export const workbenchTabComponents = {
  inbox: {
    component: PlaceholderWorkbenchTab,
    icon: "tabs/inbox",
  },
  projects: {
    component: lazy(() => import("~/projects/projects-page.tsx")),
    icon: "tabs/projects",
  },
  sessions: {
    component: lazy(() => import("~/sessions/page.tsx")),
    icon: "tabs/sessions",
  },
  search: {
    component: PlaceholderWorkbenchTab,
    icon: "tabs/search",
  },
  specs: {
    component: PlaceholderWorkbenchTab,
    icon: "tabs/spec",
  },
  tasks: {
    component: PlaceholderWorkbenchTab,
    icon: "tabs/tasks",
  },
  roadmap: {
    component: PlaceholderWorkbenchTab,
    icon: "tabs/roadmap",
  },
  settings: {
    component: lazy(() => import("~/settings/page.tsx")),
    icon: "settings",
  },
  project: {
    component: lazy(() => import("~/projects/project-page.tsx")),
    icon: "tabs/projects",
  },
  sessionChat: {
    component: lazy(() => import("~/session-chat/view.tsx")),
    icon: "tabs/sessions",
  },
  sessionChatTranscriptDebug: {
    component: lazy(() => import("~/session-chat/transcript-debug-view.tsx")),
    icon: "tabs/sessions",
  },
  terminalDebug: {
    component: lazy(() => import("~/terminal/debug-view.tsx")),
    icon: "tabs/sessions",
  },
} satisfies Record<string, WorkbenchTabDefinition>

/** The supported non-primary workbench tab kinds available in the shell. */
export type WorkbenchRegisteredTabKind = keyof typeof workbenchTabComponents

/** The supported closable workbench tab kinds available in the shell. */
export type WorkbenchDetailTabKind = WorkbenchRegisteredTabKind

/** Payload inferred from one registered non-primary workbench tab component. */
type WorkbenchTabPayload<TKind extends WorkbenchRegisteredTabKind> = ComponentProps<
  (typeof workbenchTabComponents)[TKind]["component"]
>

/** One workbench tab tracked by the shell, including the always-present main tab. */
type WorkbenchTabByKind = {
  main: {
    id: "main"
    kind: "main"
    title: string
    dirty?: undefined
    payload?: undefined
  }
} & {
  [TKind in WorkbenchRegisteredTabKind]: {
    id: string
    kind: TKind
    title: string
    dirty: boolean
    payload: WorkbenchTabPayload<TKind>
  }
}

/** The supported workbench tab kinds available in the shell. */
export type WorkbenchTabKind = keyof WorkbenchTabByKind

/** One workbench tab tracked by the shell. */
export type WorkbenchTab<TKind extends WorkbenchTabKind = WorkbenchTabKind> =
  WorkbenchTabByKind[TKind]

/** Returns the component registered for one non-primary workbench tab kind. */
export function getWorkbenchTabComponent(
  kind: WorkbenchRegisteredTabKind,
): LooseWorkbenchTabComponent {
  return workbenchTabComponents[kind].component
}

/** Returns the SVG icon registered for one workbench tab kind. */
export function getWorkbenchTabIcon(kind: WorkbenchTabKind): SvgIconName {
  return kind === "main" ? "tabs/home" : workbenchTabComponents[kind].icon
}

/** Returns whether one runtime string matches a registered closable tab kind. */
export function isWorkbenchDetailTabKind(kind: string): kind is WorkbenchDetailTabKind {
  return kind in workbenchTabComponents
}
