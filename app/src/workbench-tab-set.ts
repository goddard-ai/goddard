import { t } from "@lingui/core/macro"
import { Sigma, type Immutable } from "preact-sigma"

import type { MainTabItemId } from "./main-tab-items.ts"
import {
  createWorkbenchTab,
  getWorkbenchTabRelatedFilesystemPath,
  type WorkbenchAnyTab,
  type WorkbenchContentKind,
  type WorkbenchMainTab,
  type WorkbenchOpenTabInput,
  type WorkbenchTab,
  type WorkbenchTabKind,
} from "./workbench-tab-registry.ts"

export { getWorkbenchTabRelatedFilesystemPath }

export type {
  WorkbenchAnyTab,
  WorkbenchContentKind,
  WorkbenchMainTab,
  WorkbenchOpenTabInput,
  WorkbenchTab,
  WorkbenchTabKind,
}

/** Top-level public state owned by the workbench tab model. */
export type WorkbenchTabSetState = {
  tabs: Record<string, WorkbenchTab>
  orderedTabIds: string[]
  activeTabId: string
  recency: string[]
  navigationHistory: WorkbenchNavigationLocation[]
  navigationIndex: number
}

/** One user-visible workbench location tracked by back/forward navigation. */
export type WorkbenchNavigationLocation =
  | {
      kind: "main"
      mainTabKind: MainTabItemId
    }
  | {
      kind: "detail"
      tabId: string
    }

/** Immutable runtime value for the always-present main workbench tab. */
export const WORKBENCH_MAIN_TAB: WorkbenchMainTab = {
  id: "main",
  get title() {
    return t`Main`
  },
}

/** Maximum number of closable workbench tabs kept open at once. */
export const WORKBENCH_TAB_LIMIT = 20

function isFocusableWorkbenchTab(tabSet: WorkbenchTabSetState, tabId: string) {
  return tabId === WORKBENCH_MAIN_TAB.id || tabId in tabSet.tabs
}

function isAvailableNavigationLocation(
  tabSet: WorkbenchTabSetState,
  location: WorkbenchNavigationLocation,
) {
  return location.kind === "main" || location.tabId in tabSet.tabs
}

function areNavigationLocationsEqual(
  left: WorkbenchNavigationLocation,
  right: WorkbenchNavigationLocation,
) {
  if (left.kind !== right.kind) {
    return false
  }

  if (left.kind === "main") {
    return right.kind === "main" && left.mainTabKind === right.mainTabKind
  }

  return right.kind === "detail" && left.tabId === right.tabId
}

function findNavigableHistoryIndex(tabSet: WorkbenchTabSetState, direction: -1 | 1) {
  for (
    let index = tabSet.navigationIndex + direction;
    index >= 0 && index < tabSet.navigationHistory.length;
    index += direction
  ) {
    if (isAvailableNavigationLocation(tabSet, tabSet.navigationHistory[index])) {
      return index
    }
  }

  return null
}

function getInitialNavigationLocation(
  activeTabId: string,
  mainTabKind: MainTabItemId,
): WorkbenchNavigationLocation {
  return activeTabId === WORKBENCH_MAIN_TAB.id
    ? {
        kind: "main",
        mainTabKind,
      }
    : {
        kind: "detail",
        tabId: activeTabId,
      }
}

function findLeastRecentClosableTabId(tabSet: WorkbenchTabSetState) {
  for (let index = tabSet.recency.length - 1; index >= 0; index -= 1) {
    const tabId = tabSet.recency[index]

    if (tabId !== WORKBENCH_MAIN_TAB.id && tabId in tabSet.tabs) {
      return tabId
    }
  }

  return null
}

/** Returns the subset of a tab snapshot that should survive app reloads. */
export function getRestorableWorkbenchTabSetState(
  tabSet: Immutable<WorkbenchTabSetState>,
  mainTabKind: MainTabItemId = "inbox",
): WorkbenchTabSetState {
  const tabs = Object.fromEntries(
    Object.entries(tabSet.tabs)
      .filter(([, tab]) => tab.persistence !== "transient")
      .map(([tabId, tab]) => [tabId, tab as WorkbenchTab]),
  )
  const isFocusableTabId = (tabId: string) => tabId === WORKBENCH_MAIN_TAB.id || tabId in tabs
  const recency = tabSet.recency.filter(isFocusableTabId)
  const activeTabId = isFocusableTabId(tabSet.activeTabId)
    ? tabSet.activeTabId
    : (recency[0] ?? WORKBENCH_MAIN_TAB.id)

  return {
    tabs,
    orderedTabIds: tabSet.orderedTabIds.filter((tabId) => tabId in tabs),
    activeTabId,
    recency: [activeTabId, ...recency.filter((tabId) => tabId !== activeTabId)].filter(
      isFocusableTabId,
    ),
    navigationHistory: [getInitialNavigationLocation(activeTabId, mainTabKind)],
    navigationIndex: 0,
  }
}

/** Sigma state for the shell's closable workbench tab strip. */
export class WorkbenchTabSet extends Sigma<WorkbenchTabSetState> {
  // Close listeners keep non-persisted tab resources in sync with persisted tab records.
  #onCloseTab: (tabId: string) => void

  constructor(input: { onCloseTab?: (tabId: string) => void } = {}) {
    super({
      tabs: {},
      orderedTabIds: [],
      activeTabId: WORKBENCH_MAIN_TAB.id,
      recency: [WORKBENCH_MAIN_TAB.id],
      navigationHistory: [getInitialNavigationLocation(WORKBENCH_MAIN_TAB.id, "inbox")],
      navigationIndex: 0,
    })

    this.#onCloseTab = input.onCloseTab ?? (() => {})
  }

  /** Returns the closable tabs in their rendered order. */
  get tabList() {
    return this.orderedTabIds.map((tabId) => this.tabs[tabId]).filter(Boolean)
  }

  /** Returns the active tab, including the always-present main tab. */
  get activeTab(): WorkbenchAnyTab {
    return this.activeTabId === WORKBENCH_MAIN_TAB.id
      ? WORKBENCH_MAIN_TAB
      : (this.tabs[this.activeTabId] ?? WORKBENCH_MAIN_TAB)
  }

  /** Returns the active closable tab, when one is selected. */
  get activeClosableTab() {
    return this.activeTabId === WORKBENCH_MAIN_TAB.id ? null : (this.tabs[this.activeTabId] ?? null)
  }

  /** Returns whether back navigation can reach an available workbench location. */
  get canNavigateBack() {
    return findNavigableHistoryIndex(this, -1) !== null
  }

  /** Returns whether forward navigation can reach an available workbench location. */
  get canNavigateForward() {
    return findNavigableHistoryIndex(this, 1) !== null
  }

  /** Opens one closable tab or focuses the existing tab with the same stable id. */
  openOrFocusTab(input: WorkbenchOpenTabInput) {
    const tab = createWorkbenchTab(input)
    const previousActiveTabId = this.activeTabId

    if (this.tabs[tab.id]) {
      this.tabs[tab.id] = tab
      this.activateTab(tab.id)
      return
    }

    if (this.orderedTabIds.length >= WORKBENCH_TAB_LIMIT) {
      this.closeLeastRecentlyUsedTab()
    }

    this.tabs[tab.id] = tab
    this.orderedTabIds.push(tab.id)
    this.#focusTab(tab.id, previousActiveTabId)
    this.#recordNavigationLocation({
      kind: "detail",
      tabId: tab.id,
    })
  }

  /** Activates one visible tab and updates the recency stack used for LRU eviction. */
  activateTab(tabId: string, options: { recordHistory?: boolean } = {}) {
    const previousActiveTabId = this.activeTabId
    this.#focusTab(tabId, previousActiveTabId)

    if (options.recordHistory !== false && tabId !== WORKBENCH_MAIN_TAB.id) {
      this.#recordNavigationLocation({
        kind: "detail",
        tabId,
      })
    }
  }

  /** Activates the primary workbench tab and records its selected main view. */
  activateMainTab(mainTabKind: MainTabItemId, options: { recordHistory?: boolean } = {}) {
    const previousActiveTabId = this.activeTabId
    this.#focusTab(WORKBENCH_MAIN_TAB.id, previousActiveTabId)

    if (options.recordHistory !== false) {
      this.#recordNavigationLocation({
        kind: "main",
        mainTabKind,
      })
    }
  }

  /** Closes one closable tab and falls back to the most recently used remaining tab when needed. */
  closeTab(tabId: string) {
    const tab = this.tabs[tabId]

    if (!tab) {
      return
    }

    delete this.tabs[tabId]
    this.orderedTabIds = this.orderedTabIds.filter((id) => id !== tabId)
    this.recency = this.recency.filter((id) => id !== tabId && isFocusableWorkbenchTab(this, id))
    this.#removeDetailNavigationLocation(tabId)
    this.#onCloseTab(tabId)

    if (this.activeTabId === tabId) {
      this.activeTabId = this.recency[0] ?? WORKBENCH_MAIN_TAB.id
    }
  }

  /** Enforces the tab cap by closing the least-recently-used closable tab. */
  closeLeastRecentlyUsedTab() {
    const leastRecentTabId =
      findLeastRecentClosableTabId(this) ?? this.orderedTabIds[0] ?? WORKBENCH_MAIN_TAB.id

    if (leastRecentTabId !== WORKBENCH_MAIN_TAB.id) {
      this.closeTab(leastRecentTabId)
    }
  }

  /** Moves one visible closable tab before or after another visible closable tab. */
  moveTab(fromId: string, targetId: string, placement: "before" | "after") {
    if (fromId === targetId) {
      return
    }

    const fromIndex = this.orderedTabIds.indexOf(fromId)
    const targetIndex = this.orderedTabIds.indexOf(targetId)

    if (fromIndex < 0 || targetIndex < 0) {
      return
    }

    const nextOrder = [...this.orderedTabIds]
    nextOrder.splice(fromIndex, 1)

    const adjustedTargetIndex = targetIndex > fromIndex ? targetIndex - 1 : targetIndex
    const insertIndex = placement === "after" ? adjustedTargetIndex + 1 : adjustedTargetIndex

    if (insertIndex === fromIndex) {
      return
    }

    nextOrder.splice(insertIndex, 0, fromId)
    this.orderedTabIds = nextOrder
  }

  /** Navigates backward through available workbench locations. */
  navigateBack() {
    return this.#navigateHistory(-1)
  }

  /** Navigates forward through available workbench locations. */
  navigateForward() {
    return this.#navigateHistory(1)
  }

  #focusTab(tabId: string, previousActiveTabId: string) {
    this.activeTabId = tabId
    this.recency = [
      tabId,
      previousActiveTabId,
      ...this.recency.filter((id) => id !== tabId && id !== previousActiveTabId),
    ].filter((id) => isFocusableWorkbenchTab(this, id))
  }

  #recordNavigationLocation(location: WorkbenchNavigationLocation) {
    if (!isAvailableNavigationLocation(this, location)) {
      return
    }

    const currentLocation = this.navigationHistory[this.navigationIndex]

    if (currentLocation && areNavigationLocationsEqual(currentLocation, location)) {
      return
    }

    this.navigationHistory = [
      ...this.navigationHistory.slice(0, this.navigationIndex + 1),
      location,
    ]
    this.navigationIndex = this.navigationHistory.length - 1
  }

  #removeDetailNavigationLocation(tabId: string) {
    let removedBeforeOrAtIndex = 0
    this.navigationHistory = this.navigationHistory.filter((location, index) => {
      const keep = location.kind !== "detail" || location.tabId !== tabId

      if (!keep && index <= this.navigationIndex) {
        removedBeforeOrAtIndex += 1
      }

      return keep
    })
    this.navigationIndex = Math.max(0, this.navigationIndex - removedBeforeOrAtIndex)
  }

  #navigateHistory(direction: -1 | 1) {
    const nextIndex = findNavigableHistoryIndex(this, direction)

    if (nextIndex === null) {
      return null
    }

    const location = this.navigationHistory[nextIndex]
    const nextLocation: WorkbenchNavigationLocation =
      location.kind === "main"
        ? {
            kind: "main",
            mainTabKind: location.mainTabKind,
          }
        : {
            kind: "detail",
            tabId: location.tabId,
          }
    this.navigationIndex = nextIndex

    if (nextLocation.kind === "main") {
      this.activateMainTab(nextLocation.mainTabKind, { recordHistory: false })
    } else {
      this.activateTab(nextLocation.tabId, { recordHistory: false })
    }

    return nextLocation
  }
}

export interface WorkbenchTabSet extends WorkbenchTabSetState {}
