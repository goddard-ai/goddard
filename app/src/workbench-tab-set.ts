import { Sigma } from "preact-sigma"

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
}

/** Immutable runtime value for the always-present main workbench tab. */
export const WORKBENCH_MAIN_TAB: WorkbenchMainTab = {
  id: "main",
  title: "Main",
}

/** Maximum number of closable workbench tabs kept open at once. */
export const WORKBENCH_TAB_LIMIT = 20

function isFocusableWorkbenchTab(tabSet: WorkbenchTabSetState, tabId: string) {
  return tabId === WORKBENCH_MAIN_TAB.id || tabId in tabSet.tabs
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
    this.activeTabId = tab.id
    this.recency = [
      tab.id,
      previousActiveTabId,
      ...this.recency.filter((tabId) => tabId !== tab.id && tabId !== previousActiveTabId),
    ].filter((tabId) => isFocusableWorkbenchTab(this, tabId))
  }

  /** Activates one visible tab and updates the recency stack used for LRU eviction. */
  activateTab(tabId: string) {
    const previousActiveTabId = this.activeTabId
    this.activeTabId = tabId
    this.recency = [
      tabId,
      previousActiveTabId,
      ...this.recency.filter((id) => id !== tabId && id !== previousActiveTabId),
    ].filter((id) => isFocusableWorkbenchTab(this, id))
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
}

export interface WorkbenchTabSet extends WorkbenchTabSetState {}
