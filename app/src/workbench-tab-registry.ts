import type { DaemonSession } from "@goddard-ai/sdk"
import { lazy } from "preact/compat"

import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import type { SvgIconName } from "./lib/good-icon.tsrx"

/** One registered non-primary workbench tab definition. */
type WorkbenchTabDefinitionBase<TProps extends object> = {
  getId: (props: TProps) => string
  getTitle: (props: TProps) => string
  icon: SvgIconName
  getRelatedFilesystemPath?: (props: TProps) => string | null | undefined
  warm?: (props: TProps) => Promise<unknown>
  warmOnIdle?: (props: TProps) => Promise<unknown>
  restoreScroll?: boolean
}

type WorkbenchLazyTabDefinition<TProps extends object = any> =
  WorkbenchTabDefinitionBase<TProps> & {
    component?: never
    loadComponent: () => Promise<{ default: preact.FunctionComponent<TProps> }>
  }

type WorkbenchStaticTabDefinition<TProps extends object = any> =
  WorkbenchTabDefinitionBase<TProps> & {
    component: preact.FunctionComponent<TProps>
    loadComponent?: undefined
  }

type WorkbenchTabDefinition<TProps extends object = any> =
  | WorkbenchLazyTabDefinition<TProps>
  | WorkbenchStaticTabDefinition<TProps>

/** One loosely typed lazily rendered non-primary tab component. */
type LooseWorkbenchTabComponent = preact.FunctionComponent<any>

/** Placeholder workbench surface used until a real implementation exists. */
function PlaceholderWorkbenchTab() {
  return null
}

type InferWorkbenchTabProps<TDefinition> = TDefinition extends {
  component: preact.FunctionComponent<infer TProps>
}
  ? TProps
  : TDefinition extends {
        loadComponent: () => Promise<{ default: preact.FunctionComponent<infer TProps> }>
      }
    ? TProps
    : never

type NormalizedWorkbenchTabDefinitions<
  TDefinitions extends Record<string, WorkbenchTabDefinition>,
> = {
  [TKind in keyof TDefinitions]: TDefinitions[TKind] & {
    component: LooseWorkbenchTabComponent
  }
}

function defineWorkbenchTabs<const TDefinitions extends Record<string, WorkbenchTabDefinition>>(
  definitions: TDefinitions,
): NormalizedWorkbenchTabDefinitions<TDefinitions> {
  const normalized = Object.fromEntries(
    Object.entries(definitions).map(([kind, definition]) => {
      if (definition.loadComponent) {
        return [
          kind,
          {
            ...definition,
            component: lazy(definition.loadComponent) as LooseWorkbenchTabComponent,
          },
        ]
      }

      return [kind, definition]
    }),
  )

  return normalized as NormalizedWorkbenchTabDefinitions<TDefinitions>
}

/** Runtime registry for every non-primary workbench tab component. */
const workbenchTabDefinitions = {
  inbox: {
    loadComponent: () => import("~/inbox/page.tsrx"),
    getId: (props: { projectPath?: string }) =>
      props.projectPath
        ? `project-inbox:${encodeURIComponent(props.projectPath)}`
        : "surface:inbox",
    getTitle: (props: { projectName?: string }) =>
      props.projectName ? `Inbox · ${props.projectName}` : "Inbox",
    icon: "tabs/inbox",
    getRelatedFilesystemPath: (props: { projectPath?: string }) => props.projectPath,
  },
  projects: {
    loadComponent: () => import("~/projects/projects-page.tsrx"),
    getId: () => "surface:projects",
    getTitle: () => "Projects",
    icon: "tabs/projects",
  },
  sessions: {
    loadComponent: () => import("~/sessions/page.tsrx"),
    getId: (props: { projectPath?: string }) =>
      props.projectPath
        ? `project-sessions:${encodeURIComponent(props.projectPath)}`
        : "surface:sessions",
    getTitle: (props: { projectName?: string }) =>
      props.projectName ? `Sessions · ${props.projectName}` : "Sessions",
    icon: "tabs/sessions",
    getRelatedFilesystemPath: (props: { projectPath?: string }) => props.projectPath,
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
    loadComponent: () => import("~/settings/page.tsrx"),
    getId: () => "surface:settings",
    getTitle: () => "Settings",
    icon: "settings",
  },
  keyboardShortcuts: {
    loadComponent: () => import("~/shortcuts/view.tsrx"),
    getId: () => "workbench:keyboard-shortcuts",
    getTitle: () => "Keyboard Shortcuts",
    icon: "settings",
  },
  project: {
    loadComponent: () => import("~/projects/project-page.tsrx"),
    getId: (props: { projectPath: string }) => `project:${encodeURIComponent(props.projectPath)}`,
    getTitle: (props: { projectName?: string; projectPath: string }) =>
      props.projectName ?? props.projectPath,
    icon: "tabs/projects",
    getRelatedFilesystemPath: (props: { projectPath: string }) => props.projectPath,
  },
  sessionChat: {
    loadComponent: () => import("~/session-chat/view.tsrx"),
    getId: (props: { sessionId: string }) => `session:${props.sessionId}`,
    getTitle: (props: { sessionTitle?: string }) => props.sessionTitle ?? "Session",
    icon: "tabs/sessions",
    getRelatedFilesystemPath: (props: { relatedFilesystemPath?: string | null }) =>
      props.relatedFilesystemPath,
    warm: async (props: {
      relatedFilesystemPath: string | null
      sessionId: DaemonSession["id"]
    }) => {
      await Promise.all([
        queryClient.prefetch(goddardSdk.session.worktree.get, [{ id: props.sessionId }], {
          force: true,
        }),
        props.relatedFilesystemPath
          ? queryClient.prefetch(goddardSdk.agent.list, [
              { cwd: props.relatedFilesystemPath, includeUninstalled: true },
            ])
          : undefined,
      ])
    },
    warmOnIdle: async (props: { sessionId: DaemonSession["id"] }) => {
      await queryClient.prefetch(goddardSdk.session.history, [{ id: props.sessionId }], {
        force: true,
        refetchOnWindowReactivate: false,
      })
    },
    restoreScroll: false,
  },
  sessionChanges: {
    loadComponent: () => import("~/session-changes/view.tsrx"),
    getId: (props: { sessionId: string }) => `session-changes:${props.sessionId}`,
    getTitle: (props: { sessionTitle: string }) => `Changes · ${props.sessionTitle}`,
    icon: "tabs/changes",
  },
  pullRequest: {
    loadComponent: () => import("~/pull-requests/view.tsrx"),
    getId: (props: { pullRequestId: string }) => `pull-request:${props.pullRequestId}`,
    getTitle: (props: { pullRequestTitle?: string }) => props.pullRequestTitle ?? "Pull Request",
    icon: "tabs/pull-request",
    getRelatedFilesystemPath: (props: { relatedFilesystemPath?: string | null }) =>
      props.relatedFilesystemPath,
  },
  inboxDebug: {
    loadComponent: () => import("~/inbox/debug-view.tsrx"),
    getId: () => "debug:inbox",
    getTitle: () => "Inbox Debug",
    icon: "tabs/inbox",
  },
  terminalDebug: {
    loadComponent: () => import("~/terminal/debug-view.tsrx"),
    getId: () => "debug:terminal",
    getTitle: () => "Terminal Debug",
    icon: "tabs/sessions",
  },
  terminal: {
    component: lazy(() => import("~/terminal/view.tsrx")),
    getId: (props: { tabId: string }) => props.tabId,
    getTitle: (_props: { terminalId: string }) => "Terminal",
    icon: "tabs/sessions",
    getRelatedFilesystemPath: (props: { cwd: string | null }) => props.cwd,
    restoreScroll: false,
  },
} satisfies Record<string, WorkbenchTabDefinition>

export const workbenchTabKinds = defineWorkbenchTabs(workbenchTabDefinitions)

/** The supported non-primary workbench tab kinds available in the shell. */
type WorkbenchRegisteredTabKind = keyof typeof workbenchTabDefinitions

/** Props inferred from one registered non-primary workbench tab definition. */
type WorkbenchTabProps<TKind extends WorkbenchRegisteredTabKind> = NormalizeWorkbenchTabProps<
  InferWorkbenchTabProps<(typeof workbenchTabDefinitions)[TKind]>
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
    persistence: WorkbenchTabPersistence
    props: WorkbenchTabProps<TKind>
  }
}

/** Controls whether one detail tab should survive app reloads. */
export type WorkbenchTabPersistence = "restore" | "transient"

/** The supported closable workbench tab kinds available in the shell. */
export type WorkbenchTabKind = keyof WorkbenchTabByKind

/** One closable workbench tab tracked by the shell. */
export type WorkbenchTab<TKind extends WorkbenchTabKind = WorkbenchTabKind> =
  WorkbenchTabByKind[TKind]

/** Minimal input needed to open or focus one closable workbench tab. */
export type WorkbenchOpenTabInput<TKind extends WorkbenchTabKind = WorkbenchTabKind> = {
  [TRegisteredKind in WorkbenchTabKind]: {
    kind: TRegisteredKind
    persistence?: WorkbenchTabPersistence
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

/** Warms the lightweight resources for one workbench tab before it is activated. */
export async function warmWorkbenchTab(input: WorkbenchOpenTabInput) {
  const definition = workbenchTabDefinitions[input.kind] as WorkbenchTabDefinition<
    typeof input.props
  >

  await Promise.all([
    definition.loadComponent ? definition.loadComponent() : undefined,
    definition.warm?.(input.props),
  ])
}

/** Warms heavier resources for one workbench tab after the browser has idle time. */
export async function warmWorkbenchTabOnIdle(input: WorkbenchOpenTabInput) {
  const definition = workbenchTabDefinitions[input.kind] as WorkbenchTabDefinition<
    typeof input.props
  >

  await definition.warmOnIdle?.(input.props)
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
    persistence: input.persistence ?? "restore",
  } as WorkbenchTab
}

/** Returns whether the shell should restore raw scrollTop for one tab kind. */
export function shouldRestoreWorkbenchTabScroll(kind: WorkbenchTabKind) {
  const tabKind = workbenchTabKinds[kind]
  return "restoreScroll" in tabKind ? tabKind.restoreScroll : true
}
