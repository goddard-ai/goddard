import { Sigma } from "preact-sigma"

import {
  type WorkbenchDetailTabKind,
  type WorkbenchTab,
  type WorkbenchTabKind,
} from "./workbench-tab-registry.ts"

export type { WorkbenchTab, WorkbenchTabKind }

/** One closable workbench tab tracked by the shell. */
export type WorkbenchDetailTab = WorkbenchTab<WorkbenchDetailTabKind>

/** Top-level public state owned by the workbench tab model. */
export type WorkbenchTabSetState = {
  tabs: Record<string, WorkbenchDetailTab>
  orderedTabIds: string[]
  activeTabId: string
  recency: string[]
}

/** Immutable runtime value for the always-present primary workbench tab. */
export const WORKBENCH_PRIMARY_TAB: WorkbenchTab<"main"> = {
  id: "main",
  kind: "main",
  title: "Main",
}

/** Maximum number of closable workbench tabs kept open at once. */
export const WORKBENCH_TAB_LIMIT = 20

/** Sigma state for the shell's closable workbench tab strip. */
export class WorkbenchTabSet extends Sigma<WorkbenchTabSetState> {
  constructor() {
    super({
      tabs: {},
      orderedTabIds: [],
      activeTabId: WORKBENCH_PRIMARY_TAB.id,
      recency: [],
    })
  }

  /** Returns the closable tabs in their rendered order. */
  get tabList() {
    return this.orderedTabIds
      .map((tabId) => this.tabs[tabId])
      .filter((tab): tab is WorkbenchDetailTab => Boolean(tab))
  }

  /** Returns the active closable tab, when one is selected. */
  get activeTab() {
    return this.activeTabId === WORKBENCH_PRIMARY_TAB.id
      ? null
      : (this.tabs[this.activeTabId] ?? null)
  }

  /** Opens one closable tab or focuses the existing tab with the same stable id. */
  openOrFocusTab(tab: WorkbenchDetailTab & { payload: any }) {
    if (this.tabs[tab.id]) {
      this.tabs[tab.id] = { ...this.tabs[tab.id], ...tab }
      this.activateTab(tab.id)
      return
    }

    if (this.orderedTabIds.length >= WORKBENCH_TAB_LIMIT) {
      this.closeLeastRecentlyUsedTab()
    }

    this.tabs[tab.id] = tab
    this.orderedTabIds.push(tab.id)
    this.activeTabId = tab.id
    this.recency = [tab.id, ...this.recency.filter((tabId) => tabId !== tab.id)]
  }

  /** Activates one visible tab and updates the recency stack used for LRU eviction. */
  activateTab(tabId: string) {
    this.activeTabId = tabId

    if (tabId !== WORKBENCH_PRIMARY_TAB.id) {
      this.recency = [tabId, ...this.recency.filter((id) => id !== tabId)]
    }
  }

  /** Closes one closable tab and falls back to the primary tab when needed. */
  closeTab(tabId: string) {
    const tab = this.tabs[tabId]

    if (!tab) {
      return
    }

    delete this.tabs[tabId]
    this.orderedTabIds = this.orderedTabIds.filter((id) => id !== tabId)
    this.recency = this.recency.filter((id) => id !== tabId)

    if (this.activeTabId === tabId) {
      this.activeTabId =
        this.orderedTabIds[this.orderedTabIds.length - 1] ?? WORKBENCH_PRIMARY_TAB.id
    }
  }

  /** Enforces the tab cap by closing the least-recently-used closable tab. */
  closeLeastRecentlyUsedTab() {
    const leastRecentTabId =
      this.recency[this.recency.length - 1] ?? this.orderedTabIds[0] ?? WORKBENCH_PRIMARY_TAB.id

    if (leastRecentTabId !== WORKBENCH_PRIMARY_TAB.id) {
      this.closeTab(leastRecentTabId)
    }
  }

  /** Reorders two visible closable tabs inside the tab strip. */
  reorderTabs(fromId: string, toId: string) {
    if (fromId === toId) {
      return
    }

    const fromIndex = this.orderedTabIds.indexOf(fromId)
    const toIndex = this.orderedTabIds.indexOf(toId)

    if (fromIndex < 0 || toIndex < 0) {
      return
    }

    const nextOrder = [...this.orderedTabIds]
    nextOrder.splice(fromIndex, 1)
    nextOrder.splice(toIndex, 0, fromId)
    this.orderedTabIds = nextOrder
  }
}

export interface WorkbenchTabSet extends WorkbenchTabSetState {}
