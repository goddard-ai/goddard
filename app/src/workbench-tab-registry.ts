import { lazy } from "preact/compat"

import type { SvgIconName } from "./lib/good-icon.tsrx"

/** One registered non-primary workbench tab definition. */
type WorkbenchTabDefinition = {
  component: preact.FunctionComponent<any>
  icon: SvgIconName
  restoreScroll?: boolean
}

/** One loosely typed lazily rendered non-primary tab component. */
type LooseWorkbenchTabComponent = preact.FunctionComponent<any>

/** Placeholder workbench surface used until a real implementation exists. */
function PlaceholderWorkbenchTab() {
  return null
}

/** Runtime registry for every non-primary workbench tab component. */
export const workbenchTabKinds = {
  inbox: {
    component: lazy(() => import("~/inbox/page.tsrx")),
    icon: "tabs/inbox",
  },
  projects: {
    component: lazy(() => import("~/projects/projects-page.tsrx")),
    icon: "tabs/projects",
  },
  sessions: {
    component: lazy(() => import("~/sessions/page.tsrx")),
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
    component: lazy(() => import("~/settings/page.tsrx")),
    icon: "settings",
  },
  keyboardShortcuts: {
    component: lazy(() => import("~/shortcuts/view.tsrx")),
    icon: "settings",
  },
  project: {
    component: lazy(() => import("~/projects/project-page.tsrx")),
    icon: "tabs/projects",
  },
  sessionChat: {
    component: lazy(() => import("~/session-chat/view.tsrx")),
    icon: "tabs/sessions",
    restoreScroll: false,
  },
  sessionChanges: {
    component: lazy(() => import("~/session-changes/view.tsrx")),
    icon: "tabs/changes",
  },
  pullRequest: {
    component: lazy(() => import("~/pull-requests/view.tsrx")),
    icon: "tabs/pull-request",
  },
  inboxDebug: {
    component: lazy(() => import("~/inbox/debug-view.tsrx")),
    icon: "tabs/inbox",
  },
  sessionChatTranscriptDebug: {
    component: lazy(() => import("~/session-chat/transcript-debug-view.tsrx")),
    icon: "tabs/sessions",
    restoreScroll: false,
  },
  terminalDebug: {
    component: lazy(() => import("~/terminal/debug-view.tsrx")),
    icon: "tabs/sessions",
  },
} satisfies Record<string, WorkbenchTabDefinition>

/** The supported non-primary workbench tab kinds available in the shell. */
type WorkbenchRegisteredTabKind = keyof typeof workbenchTabKinds

/** Payload inferred from one registered non-primary workbench tab component. */
type WorkbenchTabPayload<TKind extends WorkbenchRegisteredTabKind> = NormalizeWorkbenchTabPayload<
  preact.ComponentProps<(typeof workbenchTabKinds)[TKind]["component"]>
>

/** Spreadable payload shape for tab components that infer no props. */
type NormalizeWorkbenchTabPayload<TPayload> = [TPayload] extends [never]
  ? Record<string, never>
  : TPayload extends object
    ? TPayload
    : Record<string, never>

/** One closable workbench tab tracked by the shell. */
type WorkbenchTabByKind = {
  [TKind in WorkbenchRegisteredTabKind]: {
    id: string
    kind: TKind
    title: string
    dirty: boolean
    payload: WorkbenchTabPayload<TKind>
  }
}

/** The supported closable workbench tab kinds available in the shell. */
export type WorkbenchTabKind = keyof WorkbenchTabByKind

/** One closable workbench tab tracked by the shell. */
export type WorkbenchTab<TKind extends WorkbenchTabKind = WorkbenchTabKind> =
  WorkbenchTabByKind[TKind]

/** The tab kind rendered by the active workbench content area. */
export type WorkbenchContentKind = WorkbenchRegisteredTabKind

/** The always-present main workbench tab. */
export type WorkbenchMainTab = {
  id: "main"
  title: string
}

/** Any tab tracked by the shell, including the always-present main tab. */
export type WorkbenchAnyTab = WorkbenchMainTab | WorkbenchTab

/** Returns the component registered for one non-primary workbench tab kind. */
export function getWorkbenchTabComponent(
  kind: WorkbenchRegisteredTabKind,
): LooseWorkbenchTabComponent {
  return workbenchTabKinds[kind].component
}

/** Returns the SVG icon registered for one workbench tab kind. */
export function getWorkbenchTabIcon(kind: WorkbenchTabKind): SvgIconName {
  return workbenchTabKinds[kind].icon
}

/** Returns whether the shell should restore raw scrollTop for one tab kind. */
export function shouldRestoreWorkbenchTabScroll(kind: WorkbenchTabKind) {
  const tabKind = workbenchTabKinds[kind]
  return "restoreScroll" in tabKind ? tabKind.restoreScroll : true
}
