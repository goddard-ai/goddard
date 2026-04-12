import { SigmaType } from "preact-sigma"

import { readJsonStorage, writeJsonStorage } from "~/support/workspace-storage.ts"

const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v2"

const defaultNavigationItems = [
  { id: "inbox", label: "Inbox" },
  { id: "projects", label: "Projects" },
  { id: "sessions", label: "Sessions" },
  { id: "search", label: "Search" },
  { id: "specs", label: "Specs" },
  { id: "tasks", label: "Tasks" },
  { id: "roadmap", label: "Roadmap" },
] as const satisfies {
  id: string
  label: string
}[]

/** Stable ids for the primary workbench navigation items. */
export type NavigationItemId = (typeof defaultNavigationItems)[number]["id"]

/** One item rendered in the left navigation rail. */
export type NavigationItem = {
  id: NavigationItemId
  label: string
}

/** Public state owned by the navigation model. */
type NavigationShape = {
  selectedNavId: NavigationItemId
}

/** Sigma state for the app shell's primary navigation rail. */
export const Navigation = new SigmaType<NavigationShape>("Navigation")
  .defaultState({
    selectedNavId: "inbox",
  })
  .computed({
    /** Returns the full set of primary navigation items. */
    items() {
      return defaultNavigationItems
    },

    /** Returns the currently selected primary navigation item. */
    selectedItem() {
      return this.items.find((item) => item.id === this.selectedNavId) ?? this.items[0]
    },
  })
  .actions({
    /** Selects one primary workbench view and persists it for the next launch. */
    selectNavItem(id: NavigationItemId) {
      this.selectedNavId = id
      writeJsonStorage(NAVIGATION_STORAGE_KEY, { selectedNavId: id })
    },

    /** Rehydrates the selected primary view from persisted workspace storage. */
    hydrateNavigation() {
      const snapshot = readJsonStorage<{ selectedNavId?: NavigationItemId }>(
        NAVIGATION_STORAGE_KEY,
        {},
      )
      const selectedNavId = snapshot.selectedNavId

      if (selectedNavId && this.items.some((item) => item.id === selectedNavId)) {
        this.selectedNavId = selectedNavId
      }
    },
  })

/** Runtime instance type for the navigation sigma state. */
export interface Navigation extends InstanceType<typeof Navigation> {}
