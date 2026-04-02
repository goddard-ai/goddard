/** Shared icon, navigation, and placeholder action helpers for the merged app shell. */
import type { SvgIconName } from "../support/svg-icon"
import type { NavigationItemId } from "./state/Navigation"
import type { WorkbenchTab } from "./state/WorkbenchTabRegistry"

import type { ShellIconName } from "../support/shell-icons"

/** One icon source used by closable workbench tabs in the merged shell chrome. */
export type AppShellIconSource = {
  svgName?: SvgIconName
  fallbackName?: ShellIconName
}

/** Stable action ids used by the merged shell's top bar. */
export type AppShellTopbarAction = "proposeTask" | "newSession" | "settings"

/** One navigation item's declarative shell configuration. */
type AppShellNavigationDescriptor = {
  group: "primary" | "secondary"
  icon: SvgIconName
  primaryWorkbenchTab?: Pick<WorkbenchTab<"projects">, "id" | "kind" | "payload">
}

/** Declarative shell config for every left-rail navigation item. */
export const navigationById: Record<NavigationItemId, AppShellNavigationDescriptor> = {
  projects: {
    group: "primary",
    icon: "tabs/projects",
    primaryWorkbenchTab: {
      id: "primary:projects",
      kind: "projects",
      payload: {},
    },
  },
  sessions: {
    group: "primary",
    icon: "tabs/sessions",
  },
  pullRequests: {
    group: "primary",
    icon: "tabs/pull-requests",
  },
  specs: {
    group: "secondary",
    icon: "tabs/spec",
  },
  tasks: {
    group: "secondary",
    icon: "tabs/tasks",
  },
  roadmap: {
    group: "secondary",
    icon: "tabs/roadmap",
  },
  inbox: {
    group: "secondary",
    icon: "tabs/inbox",
  },
}

/** Formats one compact rail badge count. */
export function formatBadgeCount(count: number) {
  return count > 99 ? "99+" : `${count}`
}

/** Declarative tab-icon overrides keyed by the live shell icon names. */
export const tabIconByShellIcon: Partial<Record<ShellIconName, AppShellIconSource>> = {
  main: { svgName: "tabs/home" },
  sessions: { svgName: "tabs/sessions" },
  pullRequests: { svgName: "tabs/pull-requests" },
  specs: { svgName: "tabs/spec" },
  tasks: { svgName: "tabs/tasks" },
  roadmap: { svgName: "tabs/roadmap" },
  inbox: { svgName: "tabs/inbox" },
}

/** Placeholder top-bar handlers keyed by the rendered shell action ids. */
export const topbarActionHandlerById: Record<AppShellTopbarAction, () => void> = {
  proposeTask: () => {
    // TODO: Wire this button to the real propose-task flow once that surface exists.
  },
  newSession: () => {
    // TODO: Wire this button to the real new-session flow once that surface exists.
  },
  settings: () => {
    // TODO: Wire this button to the real settings/preferences surface once it exists.
  },
}

/** Creates the ShellIcon fallback source for one tab icon without an exported SVG. */
function createFallbackTabIcon(iconName: ShellIconName): AppShellIconSource {
  // TODO: Replace this fallback once the exported shell asset set covers every live tab icon.
  return { fallbackName: iconName }
}

/** Creates one stable closable tab for a primary navigation surface when supported. */
export function createPrimaryWorkbenchTab(
  navId: NavigationItemId,
  title: string,
  icon: WorkbenchTab<"projects">["icon"],
) {
  const primaryWorkbenchTab = navigationById[navId].primaryWorkbenchTab

  if (!primaryWorkbenchTab) {
    return null
  }

  const tab: WorkbenchTab<"projects"> = {
    ...primaryWorkbenchTab,
    title,
    icon,
    dirty: false,
  }

  return tab
}

/** Maps one live tab icon onto the merged shell's icon set. */
export function getTabIcon(iconName: ShellIconName): AppShellIconSource {
  return tabIconByShellIcon[iconName] ?? createFallbackTabIcon(iconName)
}

/** Handles the current placeholder top-bar actions without changing live state yet. */
export function handleTopbarAction(action: AppShellTopbarAction) {
  topbarActionHandlerById[action]()
}
