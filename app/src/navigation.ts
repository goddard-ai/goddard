import { Sigma } from "preact-sigma"

import { defaultNavigationItems, type NavigationItemId } from "./navigation-items.ts"

/** Public state owned by the navigation model. */
export type NavigationState = {
  selectedNavId: NavigationItemId
}

/** Returns whether one runtime value is a supported primary navigation id. */
export function isNavigationItemId(value: unknown): value is NavigationItemId {
  return typeof value === "string" && defaultNavigationItems.some((item) => item.id === value)
}

/** Sigma state for the app shell's primary navigation rail. */
export class Navigation extends Sigma<NavigationState> {
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

  /** Selects one primary workbench view. */
  selectNavItem(id: NavigationItemId) {
    this.selectedNavId = id
  }
}

export interface Navigation extends NavigationState {}
