import { SigmaType } from "preact-sigma"
import type { ShellIconName } from "../support/shell-icons"
import { readJsonStorage, writeJsonStorage } from "../support/workspace-storage"

const WORKBENCH_TABS_STORAGE_KEY = "goddard.app.workbench-tabs.v2"

/** The supported detail-tab kinds available during sprint 1. */
export type WorkbenchDetailTabKind = "project"

/** Payload used by a project detail tab. */
export type ProjectWorkbenchTabPayload = {
  projectPath: string
}

/** Union payload carried by one detail tab. */
export type WorkbenchDetailTabPayload = ProjectWorkbenchTabPayload

/** Immutable descriptor for the always-present primary tab. */
export type WorkbenchPrimaryTab = {
  id: "main"
  title: string
  icon: ShellIconName
}

/** One closable detail tab tracked by the shell. */
export type WorkbenchDetailTab = {
  id: string
  kind: WorkbenchDetailTabKind
  title: string
  icon: ShellIconName
  payload: WorkbenchDetailTabPayload
  dirty: boolean
}

/** Persisted snapshot for open workbench detail tabs. */
type WorkbenchTabsSnapshot = {
  detailTabs: WorkbenchDetailTab[]
  activeTabId: string
  recency: string[]
}

/** Top-level public state owned by the workbench tab model. */
type WorkbenchTabSetShape = {
  detailTabs: Record<string, WorkbenchDetailTab>
  orderedDetailTabIds: string[]
  activeTabId: string
  recency: string[]
}

export const WORKBENCH_PRIMARY_TAB: WorkbenchPrimaryTab = {
  id: "main",
  title: "Main",
  icon: "main",
}

export const WORKBENCH_DETAIL_TAB_LIMIT = 20

/** Persists the current tab layout into local workspace storage. */
function persistTabs(state: WorkbenchTabSetShape): void {
  writeJsonStorage(WORKBENCH_TABS_STORAGE_KEY, {
    detailTabs: state.orderedDetailTabIds
      .map((tabId) => state.detailTabs[tabId])
      .filter((tab): tab is WorkbenchDetailTab => Boolean(tab)),
    activeTabId: state.activeTabId,
    recency: state.recency,
  })
}

/** Sigma state for the shell's closable detail tab strip. */
export const WorkbenchTabSet = new SigmaType<WorkbenchTabSetShape>("WorkbenchTabSet")
  .defaultState({
    detailTabs: {},
    orderedDetailTabIds: [],
    activeTabId: WORKBENCH_PRIMARY_TAB.id,
    recency: [],
  })
  .computed({
    /** Returns the detail tabs in their rendered order. */
    detailTabList() {
      return this.orderedDetailTabIds
        .map((tabId) => this.detailTabs[tabId])
        .filter((tab): tab is WorkbenchDetailTab => Boolean(tab))
    },

    /** Returns the active detail tab, when one is selected. */
    activeDetailTab() {
      return this.activeTabId === WORKBENCH_PRIMARY_TAB.id
        ? null
        : (this.detailTabs[this.activeTabId] ?? null)
    },
  })
  .actions({
    /** Opens one detail tab or focuses the existing tab with the same stable id. */
    openOrFocusTab(tab: WorkbenchDetailTab) {
      if (this.detailTabs[tab.id]) {
        this.detailTabs[tab.id] = { ...this.detailTabs[tab.id], ...tab }
        this.activateTab(tab.id)
        persistTabs(this)
        return
      }

      if (this.orderedDetailTabIds.length >= WORKBENCH_DETAIL_TAB_LIMIT) {
        this.closeLeastRecentlyUsedTab()
      }

      this.detailTabs[tab.id] = tab
      this.orderedDetailTabIds.push(tab.id)
      this.activeTabId = tab.id
      this.recency = [tab.id, ...this.recency.filter((tabId) => tabId !== tab.id)]
      persistTabs(this)
    },

    /** Activates one visible tab and updates the recency stack used for LRU eviction. */
    activateTab(tabId: string) {
      this.activeTabId = tabId

      if (tabId !== WORKBENCH_PRIMARY_TAB.id) {
        this.recency = [tabId, ...this.recency.filter((id) => id !== tabId)]
      }

      persistTabs(this)
    },

    /** Closes one detail tab and falls back to the primary tab when needed. */
    closeTab(tabId: string) {
      const detailTab = this.detailTabs[tabId]

      if (!detailTab) {
        return
      }

      delete this.detailTabs[tabId]
      this.orderedDetailTabIds = this.orderedDetailTabIds.filter((id) => id !== tabId)
      this.recency = this.recency.filter((id) => id !== tabId)

      if (this.activeTabId === tabId) {
        this.activeTabId =
          this.orderedDetailTabIds[this.orderedDetailTabIds.length - 1] ?? WORKBENCH_PRIMARY_TAB.id
      }

      persistTabs(this)
    },

    /** Enforces the tab cap by closing the least-recently-used detail tab. */
    closeLeastRecentlyUsedTab() {
      const leastRecentTabId =
        this.recency[this.recency.length - 1] ??
        this.orderedDetailTabIds[0] ??
        WORKBENCH_PRIMARY_TAB.id

      if (leastRecentTabId !== WORKBENCH_PRIMARY_TAB.id) {
        this.closeTab(leastRecentTabId)
      }
    },

    /** Reorders two visible detail tabs inside the tab strip. */
    reorderTabs(fromId: string, toId: string) {
      if (fromId === toId) {
        return
      }

      const fromIndex = this.orderedDetailTabIds.indexOf(fromId)
      const toIndex = this.orderedDetailTabIds.indexOf(toId)

      if (fromIndex < 0 || toIndex < 0) {
        return
      }

      const nextOrder = [...this.orderedDetailTabIds]
      nextOrder.splice(fromIndex, 1)
      nextOrder.splice(toIndex, 0, fromId)
      this.orderedDetailTabIds = nextOrder
      persistTabs(this)
    },

    /** Rehydrates open tabs from local workspace storage after the shell boots. */
    hydrateTabsFromStore() {
      const snapshot = readJsonStorage<WorkbenchTabsSnapshot>(WORKBENCH_TABS_STORAGE_KEY, {
        detailTabs: [],
        activeTabId: WORKBENCH_PRIMARY_TAB.id,
        recency: [],
      })

      const detailTabs = Object.fromEntries(snapshot.detailTabs.map((tab) => [tab.id, tab]))
      const orderedDetailTabIds = snapshot.detailTabs.map((tab) => tab.id)
      const activeTabId =
        snapshot.activeTabId === WORKBENCH_PRIMARY_TAB.id || detailTabs[snapshot.activeTabId]
          ? snapshot.activeTabId
          : WORKBENCH_PRIMARY_TAB.id

      this.detailTabs = detailTabs
      this.orderedDetailTabIds = orderedDetailTabIds
      this.activeTabId = activeTabId
      this.recency = snapshot.recency.filter((tabId) => Boolean(detailTabs[tabId]))
    },
  })

/** Runtime instance type for the workbench tab sigma state. */
export interface WorkbenchTabSet extends InstanceType<typeof WorkbenchTabSet> {}
