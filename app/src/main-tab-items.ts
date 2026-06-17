export type MainTabItemGroup = "primary" | "secondary"

/** Static main tab metadata shared by the model and app shell. */
export const defaultMainTabItems = [
  { id: "inbox", label: "Inbox", group: "primary" },
  { id: "sessions", label: "Sessions", group: "primary" },
  { id: "pipelines", label: "Pipelines", group: "primary" },
  { id: "search", label: "Search", group: "primary" },
  { id: "specs", label: "Specs", group: "secondary" },
  { id: "tasks", label: "Tasks", group: "secondary" },
  { id: "roadmap", label: "Roadmap", group: "secondary" },
] as const satisfies {
  group: MainTabItemGroup
  id: string
  label: string
}[]

/** Stable ids for the main tab items. */
export type MainTabItemId = (typeof defaultMainTabItems)[number]["id"]

/** One item rendered in the left sidebar. */
export type MainTabItem = (typeof defaultMainTabItems)[number]
