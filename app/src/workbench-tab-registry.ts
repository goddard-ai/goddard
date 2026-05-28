import { inboxAppPlugin } from "@goddard-ai/inbox/app"
import { lazy } from "preact/compat"

import type { SvgIconName } from "./lib/good-icon.tsrx"

/** One registered non-primary workbench tab definition. */
type WorkbenchTabDefinition<TPayload extends object = any> = {
  component: preact.FunctionComponent<any>
  getId: (payload: TPayload) => string
  getTitle: (payload: TPayload) => string
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
    getId: () => "surface:inbox",
    getTitle: () => "Inbox",
    icon: inboxAppPlugin.workbenchTab.icon,
  },
  projects: {
    component: lazy(() => import("~/projects/projects-page.tsrx")),
    getId: () => "surface:projects",
    getTitle: () => "Projects",
    icon: "tabs/projects",
  },
  sessions: {
    component: lazy(() => import("~/sessions/page.tsrx")),
    getId: () => "surface:sessions",
    getTitle: () => "Sessions",
    icon: "tabs/sessions",
  },
  search: {
    component: PlaceholderWorkbenchTab,
    getId: () => "surface:search",
    getTitle: () => "Search",
    icon: "tabs/search",
  },
  specs: {
    component: PlaceholderWorkbenchTab,
    getId: () => "surface:specs",
    getTitle: () => "Specs",
    icon: "tabs/spec",
  },
  tasks: {
    component: PlaceholderWorkbenchTab,
    getId: () => "surface:tasks",
    getTitle: () => "Tasks",
    icon: "tabs/tasks",
  },
  roadmap: {
    component: PlaceholderWorkbenchTab,
    getId: () => "surface:roadmap",
    getTitle: () => "Roadmap",
    icon: "tabs/roadmap",
  },
  settings: {
    component: lazy(() => import("~/settings/page.tsrx")),
    getId: () => "surface:settings",
    getTitle: () => "Settings",
    icon: "settings",
  },
  keyboardShortcuts: {
    component: lazy(() => import("~/shortcuts/view.tsrx")),
    getId: () => "workbench:keyboard-shortcuts",
    getTitle: () => "Keyboard Shortcuts",
    icon: "settings",
  },
  project: {
    component: lazy(() => import("~/projects/project-page.tsrx")),
    getId: (payload: { projectPath: string }) =>
      `project:${encodeURIComponent(payload.projectPath)}`,
    getTitle: (payload: { projectName?: string; projectPath: string }) =>
      payload.projectName ?? payload.projectPath,
    icon: "tabs/projects",
  },
  sessionChat: {
    component: lazy(() => import("~/session-chat/view.tsrx")),
    getId: (payload: { sessionId: string }) => `session:${payload.sessionId}`,
    getTitle: (payload: { sessionTitle?: string }) => payload.sessionTitle ?? "Session",
    icon: "tabs/sessions",
    restoreScroll: false,
  },
  sessionChanges: {
    component: lazy(() => import("~/session-changes/view.tsrx")),
    getId: (payload: { sessionId: string }) => `session-changes:${payload.sessionId}`,
    getTitle: (payload: { sessionTitle: string }) => `Changes · ${payload.sessionTitle}`,
    icon: "tabs/changes",
  },
  pullRequest: {
    component: lazy(() => import("~/pull-requests/view.tsrx")),
    getId: (payload: { pullRequestId: string }) => `pull-request:${payload.pullRequestId}`,
    getTitle: (payload: { pullRequestTitle?: string }) =>
      payload.pullRequestTitle ?? "Pull Request",
    icon: "tabs/pull-request",
  },
  inboxDebug: {
    component: lazy(() => import("~/inbox/debug-view.tsrx")),
    getId: () => "debug:inbox",
    getTitle: () => "Inbox Debug",
    icon: "tabs/inbox",
  },
  sessionChatTranscriptDebug: {
    component: lazy(() => import("~/session-chat/transcript-debug-view.tsrx")),
    getId: () => "debug:session-chat-transcript",
    getTitle: () => "Session Chat Debug",
    icon: "tabs/sessions",
    restoreScroll: false,
  },
  terminalDebug: {
    component: lazy(() => import("~/terminal/debug-view.tsrx")),
    getId: () => "debug:terminal",
    getTitle: () => "Terminal Debug",
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

/** Minimal input needed to open or focus one closable workbench tab. */
export type WorkbenchOpenTabInput<TKind extends WorkbenchTabKind = WorkbenchTabKind> = {
  [TRegisteredKind in WorkbenchTabKind]: {
    kind: TRegisteredKind
    payload: WorkbenchTabPayload<TRegisteredKind>
  }
}[TKind]

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

/** Derives the full stored tab record from the minimal caller-owned tab input. */
export function createWorkbenchTab(input: WorkbenchOpenTabInput): WorkbenchTab {
  const definition = workbenchTabKinds[input.kind] as WorkbenchTabDefinition<typeof input.payload>

  return {
    id: definition.getId(input.payload),
    kind: input.kind,
    title: definition.getTitle(input.payload),
    payload: input.payload,
    dirty: false,
  } as WorkbenchTab
}

/** Returns whether the shell should restore raw scrollTop for one tab kind. */
export function shouldRestoreWorkbenchTabScroll(kind: WorkbenchTabKind) {
  const tabKind = workbenchTabKinds[kind]
  return "restoreScroll" in tabKind ? tabKind.restoreScroll : true
}
