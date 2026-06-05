import { inboxAppPlugin } from "@goddard-ai/inbox/app"
import { lazy } from "preact/compat"

import type { SvgIconName } from "./lib/good-icon.tsrx"

/** One registered non-primary workbench tab definition. */
type WorkbenchTabDefinition<TProps extends object = any> = {
  component: preact.FunctionComponent<any>
  getId: (props: TProps) => string
  getTitle: (props: TProps) => string
  icon: SvgIconName
  getRelatedFilesystemPath?: (props: TProps) => string | null | undefined
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
    getId: (props: { projectPath: string }) => `project:${encodeURIComponent(props.projectPath)}`,
    getTitle: (props: { projectName?: string; projectPath: string }) =>
      props.projectName ?? props.projectPath,
    icon: "tabs/projects",
    getRelatedFilesystemPath: (props: { projectPath: string }) => props.projectPath,
  },
  sessionChat: {
    component: lazy(() => import("~/session-chat/view.tsrx")),
    getId: (props: { sessionId: string }) => `session:${props.sessionId}`,
    getTitle: (props: { sessionTitle?: string }) => props.sessionTitle ?? "Session",
    icon: "tabs/sessions",
    getRelatedFilesystemPath: (props: { relatedFilesystemPath?: string | null }) =>
      props.relatedFilesystemPath,
    restoreScroll: false,
  },
  sessionChanges: {
    component: lazy(() => import("~/session-changes/view.tsrx")),
    getId: (props: { sessionId: string }) => `session-changes:${props.sessionId}`,
    getTitle: (props: { sessionTitle: string }) => `Changes · ${props.sessionTitle}`,
    icon: "tabs/changes",
  },
  pullRequest: {
    component: lazy(() => import("~/pull-requests/view.tsrx")),
    getId: (props: { pullRequestId: string }) => `pull-request:${props.pullRequestId}`,
    getTitle: (props: { pullRequestTitle?: string }) => props.pullRequestTitle ?? "Pull Request",
    icon: "tabs/pull-request",
    getRelatedFilesystemPath: (props: { relatedFilesystemPath?: string | null }) =>
      props.relatedFilesystemPath,
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

/** Props inferred from one registered non-primary workbench tab component. */
type WorkbenchTabProps<TKind extends WorkbenchRegisteredTabKind> = NormalizeWorkbenchTabProps<
  preact.ComponentProps<(typeof workbenchTabKinds)[TKind]["component"]>
>

/** Spreadable props shape for tab components that infer no props. */
type NormalizeWorkbenchTabProps<TProps> = [TProps] extends [never]
  ? Record<string, never>
  : TProps extends object
    ? TProps
    : Record<string, never>

/** One closable workbench tab tracked by the shell. */
type WorkbenchTabByKind = {
  [TKind in WorkbenchRegisteredTabKind]: {
    id: string
    kind: TKind
    title: string
    dirty: boolean
    props: WorkbenchTabProps<TKind>
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
    props: WorkbenchTabProps<TRegisteredKind>
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

/** Resolves the filesystem path one workbench tab is associated with, when the tab kind declares one. */
export function getWorkbenchTabRelatedFilesystemPath(tab: WorkbenchTab) {
  const definition = workbenchTabKinds[tab.kind] as WorkbenchTabDefinition<typeof tab.props>
  return definition.getRelatedFilesystemPath?.(tab.props) ?? null
}

/** Derives the full stored tab record from the minimal caller-owned tab input. */
export function createWorkbenchTab(input: WorkbenchOpenTabInput): WorkbenchTab {
  const definition = workbenchTabKinds[input.kind] as WorkbenchTabDefinition<typeof input.props>

  return {
    id: definition.getId(input.props),
    kind: input.kind,
    title: definition.getTitle(input.props),
    props: input.props,
    dirty: false,
  } as WorkbenchTab
}

/** Returns whether the shell should restore raw scrollTop for one tab kind. */
export function shouldRestoreWorkbenchTabScroll(kind: WorkbenchTabKind) {
  const tabKind = workbenchTabKinds[kind]
  return "restoreScroll" in tabKind ? tabKind.restoreScroll : true
}
