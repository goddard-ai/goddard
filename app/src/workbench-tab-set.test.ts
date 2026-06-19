import { expect, test } from "vitest"

import { WORKBENCH_MAIN_TAB, WORKBENCH_TAB_LIMIT, WorkbenchTabSet } from "./workbench-tab-set.ts"

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
