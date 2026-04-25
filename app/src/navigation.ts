import { Sigma } from "preact-sigma"

import { readJsonStorage, writeJsonStorage } from "~/support/workspace-storage.ts"

const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v2"

const defaultNavigationItems = [
  { id: "inbox", label: "Inbox" },
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
export class Navigation extends Sigma<NavigationShape> {
  declare selectedNavId: NavigationItemId

  constructor() {
    super({
      selectedNavId: "inbox",
    })
  }

  /** Returns the full set of primary navigation items. */
  get items() {
    return defaultNavigationItems
  }

  /** Returns the currently selected primary navigation item. */
  get selectedItem() {
    return this.items.find((item) => item.id === this.selectedNavId) ?? this.items[0]
  }

  /** Selects one primary workbench view and persists it for the next launch. */
  selectNavItem(id: NavigationItemId) {
    this.selectedNavId = id
    writeJsonStorage(NAVIGATION_STORAGE_KEY, { selectedNavId: id })
  }

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
  }
}
