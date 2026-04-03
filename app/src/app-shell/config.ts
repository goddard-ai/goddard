import type { NavigationItemId } from "~/navigation.ts"

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
    tabKinds: ["inbox", "projects", "sessions", "search"],
  },
  {
    group: "secondary",
    tabKinds: ["specs", "tasks", "roadmap"],
  },
]
