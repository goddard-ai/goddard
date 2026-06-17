import { text } from "~/language/text.ts"

export type MainTabItemGroup = "primary" | "secondary"

/** Static main tab metadata shared by the model and app shell. */
export const defaultMainTabItems = [
  { id: "inbox", label: text.inbox, group: "primary" },
  { id: "sessions", label: text.sessions, group: "primary" },
  { id: "search", label: text.search, group: "primary" },
  { id: "specs", label: text.specs, group: "secondary" },
  { id: "tasks", label: text.tasks, group: "secondary" },
  { id: "roadmap", label: text.roadmap, group: "secondary" },
] as const satisfies {
  group: MainTabItemGroup
  id: string
  label: string
}[]

/** Stable ids for the main tab items. */
export type MainTabItemId = (typeof defaultMainTabItems)[number]["id"]

/** One item rendered in the left sidebar. */
export type MainTabItem = (typeof defaultMainTabItems)[number]
