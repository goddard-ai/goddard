import type { NavigationItemId } from "~/navigation.ts"

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
