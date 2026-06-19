import { t } from "@lingui/core/macro"

export type MainTabItemGroup = "primary" | "secondary"

/** Static main tab metadata shared by the model and app shell. */
export const defaultMainTabItems = [
  {
    id: "inbox",
    get label() {
      return t`Inbox`
    },
    group: "primary",
  },
  {
    id: "sessions",
    get label() {
      return t`Sessions`
    },
    group: "primary",
  },
  {
    id: "search",
    get label() {
      return t`Search`
    },
    group: "primary",
  },
  {
    id: "specs",
    get label() {
      return t`Specs`
    },
    group: "secondary",
  },
  {
    id: "tasks",
    get label() {
      return t`Tasks`
    },
    group: "secondary",
  },
  {
    id: "roadmap",
    get label() {
      return t`Roadmap`
    },
    group: "secondary",
  },
] as const satisfies {
  group: MainTabItemGroup
  id: string
  label: string
}[]

/** Stable ids for the main tab items. */
export type MainTabItemId = (typeof defaultMainTabItems)[number]["id"]

/** One item rendered in the left sidebar. */
export type MainTabItem = (typeof defaultMainTabItems)[number]
