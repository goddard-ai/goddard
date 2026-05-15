import { Sigma } from "preact-sigma"

import { defaultMainTabItems, type MainTabItemId } from "./main-tab-items.ts"

/** Public state owned by the main tab model. */
export type MainTabState = {
  selectedKind: MainTabItemId
}

/** Returns whether one runtime value is a supported main tab kind. */
export function isMainTabItemId(value: unknown): value is MainTabItemId {
  return typeof value === "string" && defaultMainTabItems.some((item) => item.id === value)
}

/** Sigma state for the app shell's main tab selector. */
export class MainTab extends Sigma<MainTabState> {
  constructor() {
    super({
      selectedKind: "inbox",
    })
  }

  /** Returns the full set of main tab items. */
  get items() {
    return defaultMainTabItems
  }

  /** Returns the currently selected main tab item. */
  get selectedItem() {
    return this.items.find((item) => item.id === this.selectedKind) ?? this.items[0]
  }

  /** Selects one main tab view. */
  selectKind(id: MainTabItemId) {
    this.selectedKind = id
  }
}

export interface MainTab extends MainTabState {}
