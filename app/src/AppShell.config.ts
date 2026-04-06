import type { NavigationItemId } from "./navigation"

/** Stable action ids used by the merged shell's top bar. */
export type AppShellTopbarAction = "proposeTask" | "newSession" | "settings"

/** One grouped section in the left app shell rail. */
type AppShellSection = {
  group: "primary" | "secondary"
  tabKinds: readonly NavigationItemId[]
}

/** Ordered left-rail sections rendered by the merged app shell. */
export const appShellSections: readonly AppShellSection[] = [
  {
    group: "primary",
    tabKinds: ["inbox", "sessions", "search"],
  },
  {
    group: "secondary",
    tabKinds: ["specs", "tasks", "roadmap"],
  },
]

/** Formats one compact rail badge count. */
export function formatBadgeCount(count: number) {
  return count > 99 ? "99+" : `${count}`
}
