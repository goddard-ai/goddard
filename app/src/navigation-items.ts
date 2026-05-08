import { inboxAppPlugin } from "@goddard-ai/inbox/app"

export type NavigationItemGroup = "primary" | "secondary"

/** Static workbench navigation metadata shared by the model and app shell. */
export const defaultNavigationItems = [
  {
    id: inboxAppPlugin.navigation.id,
    label: inboxAppPlugin.navigation.label,
    group: "primary",
  },
  { id: "sessions", label: "Sessions", group: "primary" },
  { id: "search", label: "Search", group: "primary" },
  { id: "specs", label: "Specs", group: "secondary" },
  { id: "tasks", label: "Tasks", group: "secondary" },
  { id: "roadmap", label: "Roadmap", group: "secondary" },
] as const satisfies {
  group: NavigationItemGroup
  id: string
  label: string
}[]

/** Stable ids for the primary workbench navigation items. */
export type NavigationItemId = (typeof defaultNavigationItems)[number]["id"]

/** One item rendered in the left navigation rail. */
export type NavigationItem = (typeof defaultNavigationItems)[number]
