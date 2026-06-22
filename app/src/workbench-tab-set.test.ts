import { expect, test } from "vitest"

import {
  getRestorableWorkbenchTabSetState,
  WORKBENCH_MAIN_TAB,
  WORKBENCH_TAB_LIMIT,
  WorkbenchTabSet,
} from "./workbench-tab-set.ts"

function openSessionTabs(tabSet: WorkbenchTabSet, count: number) {
  for (let index = 1; index <= count; index += 1) {
    tabSet.openOrFocusTab({
      kind: "sessionChat",
      props: {
        relatedFilesystemPath: null,
        sessionId: `session-${index}`,
      },
    } as any)
  }
}

test("WorkbenchTabSet reports directly closed tabs", () => {
  const closedTabIds: string[] = []
  const tabSet = new WorkbenchTabSet({
    onCloseTab: (tabId) => {
      closedTabIds.push(tabId)
    },
  })

  tabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "session-1",
    },
  } as any)
  tabSet.closeTab("session:session-1")

  expect(closedTabIds).toEqual(["session:session-1"])
})

test("WorkbenchTabSet opens restorable tabs by default", () => {
  const tabSet = new WorkbenchTabSet()

  tabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "session-1",
    },
  } as any)

  expect(tabSet.tabs["session:session-1"]?.persistence).toBe("restore")
})

test("WorkbenchTabSet focuses the most recently used open tab after closing the active tab", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 3)

  tabSet.activateTab("session:session-1")
  tabSet.activateTab("session:session-3")
  tabSet.closeTab("session:session-3")

  expect(tabSet.activeTabId).toBe("session:session-1")
})

test("WorkbenchTabSet can refocus the main tab after closing the active tab", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 2)
  tabSet.activateTab(WORKBENCH_MAIN_TAB.id)
  tabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "session-3",
    },
  } as any)
  tabSet.closeTab("session:session-3")

  expect(tabSet.activeTabId).toBe(WORKBENCH_MAIN_TAB.id)
})

test("WorkbenchTabSet navigates backward and forward between main workbench locations", () => {
  const tabSet = new WorkbenchTabSet()

  tabSet.activateMainTab("sessions")
  tabSet.activateMainTab("search")

  expect(tabSet.canNavigateBack).toBe(true)
  expect(tabSet.canNavigateForward).toBe(false)

  expect(tabSet.navigateBack()).toEqual({
    kind: "main",
    mainTabKind: "sessions",
  })
  expect(tabSet.activeTabId).toBe(WORKBENCH_MAIN_TAB.id)
  expect(tabSet.canNavigateForward).toBe(true)

  expect(tabSet.navigateForward()).toEqual({
    kind: "main",
    mainTabKind: "search",
  })
})

test("WorkbenchTabSet clears forward navigation after normal navigation", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 2)
  expect(tabSet.navigateBack()).toEqual({
    kind: "detail",
    tabId: "session:session-1",
  })

  tabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "session-3",
    },
  } as any)

  expect(tabSet.canNavigateForward).toBe(false)
  expect(tabSet.navigateBack()).toEqual({
    kind: "detail",
    tabId: "session:session-1",
  })
})

test("WorkbenchTabSet skips closed detail tabs during navigation", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 2)
  expect(tabSet.navigateBack()).toEqual({
    kind: "detail",
    tabId: "session:session-1",
  })

  tabSet.closeTab("session:session-2")

  expect(tabSet.canNavigateForward).toBe(false)
  expect(tabSet.navigateForward()).toBeNull()
})

test("WorkbenchTabSet moves a tab before another visible tab", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 3)
  tabSet.moveTab("session:session-3", "session:session-1", "before")

  expect(tabSet.orderedTabIds).toEqual([
    "session:session-3",
    "session:session-1",
    "session:session-2",
  ])
})

test("WorkbenchTabSet moves a tab after another visible tab", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 3)
  tabSet.moveTab("session:session-1", "session:session-3", "after")

  expect(tabSet.orderedTabIds).toEqual([
    "session:session-2",
    "session:session-3",
    "session:session-1",
  ])
})

test("WorkbenchTabSet ignores no-op tab moves", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 3)
  tabSet.moveTab("session:session-2", "session:session-1", "after")
  tabSet.moveTab("session:session-2", "session:session-3", "before")
  tabSet.moveTab("session:session-2", "session:session-2", "after")

  expect(tabSet.orderedTabIds).toEqual([
    "session:session-1",
    "session:session-2",
    "session:session-3",
  ])
})

test("WorkbenchTabSet ignores moves with unknown tabs", () => {
  const tabSet = new WorkbenchTabSet()

  openSessionTabs(tabSet, 2)
  tabSet.moveTab("session:missing", "session:session-1", "before")
  tabSet.moveTab("session:session-1", "session:missing", "after")

  expect(tabSet.orderedTabIds).toEqual(["session:session-1", "session:session-2"])
})

test("WorkbenchTabSet reports least-recently-used tabs closed by the tab limit", () => {
  const closedTabIds: string[] = []
  const tabSet = new WorkbenchTabSet({
    onCloseTab: (tabId) => {
      closedTabIds.push(tabId)
    },
  })

  for (let index = 0; index <= WORKBENCH_TAB_LIMIT; index += 1) {
    tabSet.openOrFocusTab({
      kind: "sessionChat",
      props: {
        relatedFilesystemPath: null,
        sessionId: `session-${index}`,
      },
    } as any)
  }

  expect(closedTabIds).toEqual(["session:session-0"])
  expect(tabSet.tabs["session:session-0"]).toBeUndefined()
})

test("getRestorableWorkbenchTabSetState removes transient tabs from reload snapshots", () => {
  const tabSet = new WorkbenchTabSet()

  tabSet.openOrFocusTab({
    kind: "sessionChat",
    props: {
      relatedFilesystemPath: null,
      sessionId: "session-1",
    },
  } as any)
  tabSet.openOrFocusTab({
    kind: "sessionChat",
    persistence: "transient",
    props: {
      relatedFilesystemPath: null,
      sessionId: "session-2",
    },
  } as any)

  expect(getRestorableWorkbenchTabSetState(tabSet)).toMatchObject({
    activeTabId: "session:session-1",
    orderedTabIds: ["session:session-1"],
    recency: ["session:session-1", WORKBENCH_MAIN_TAB.id],
    tabs: {
      "session:session-1": {
        persistence: "restore",
      },
    },
  })
})
