import { SigmaType } from "preact-sigma"
import type { ShellIconName } from "../../support/shell-icons"
import { readJsonStorage, writeJsonStorage } from "../../support/workspace-storage"

const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v2"

/** Stable ids for the primary workbench navigation items. */
export type NavigationItemId = "inbox" | "sessions" | "search" | "specs" | "tasks" | "roadmap"

/** One item rendered in the left navigation rail. */
export type NavigationItem = {
  id: NavigationItemId
  icon: ShellIconName
  label: string
  ariaLabel: string
}

/** Badge counts keyed by primary navigation id. */
type NavigationBadgeCounts = Partial<Record<NavigationItemId, number>>

/** Public state owned by the navigation model. */
type NavigationShape = {
  selectedNavId: NavigationItemId
  badgeCounts: NavigationBadgeCounts
}

const defaultNavigationItems: NavigationItem[] = [
  { id: "inbox", icon: "inbox", label: "Inbox", ariaLabel: "Inbox" },
  { id: "sessions", icon: "sessions", label: "Sessions", ariaLabel: "Sessions" },
  { id: "search", icon: "search", label: "Search", ariaLabel: "Search" },
  { id: "specs", icon: "specs", label: "Specs", ariaLabel: "Specs" },
  { id: "tasks", icon: "tasks", label: "Tasks", ariaLabel: "Tasks" },
  { id: "roadmap", icon: "roadmap", label: "Roadmap", ariaLabel: "Roadmap" },
]

/** Sigma state for the app shell's primary navigation rail. */
export const Navigation = new SigmaType<NavigationShape>("Navigation")
  .defaultState({
    selectedNavId: "inbox",
    badgeCounts: {},
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

    /** Updates one badge count without forcing sidebar consumers to compute cross-domain data. */
    setBadgeCount(id: NavigationItemId, count: number) {
      if (count <= 0) {
        delete this.badgeCounts[id]
        return
      }

      this.badgeCounts[id] = count
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
