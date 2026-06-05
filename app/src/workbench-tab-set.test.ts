import { expect, test } from "bun:test"

import { WORKBENCH_TAB_LIMIT, WorkbenchTabSet } from "./workbench-tab-set.ts"

test("WorkbenchTabSet reports directly closed tabs", () => {
  const closedTabIds: string[] = []
  const tabSet = new WorkbenchTabSet({
    onCloseTab: (tabId) => {
      closedTabIds.push(tabId)
    },
  })

  tabSet.openOrFocusTab({
    kind: "sessionChat",
    payload: {
      relatedFilesystemPath: null,
      sessionId: "session-1",
    },
  } as any)
  tabSet.closeTab("session:session-1")

  expect(closedTabIds).toEqual(["session:session-1"])
})

test("WorkbenchTabSet focuses the most recently used open tab after closing the active tab", () => {
  const tabSet = new WorkbenchTabSet()

  for (let index = 1; index <= 3; index += 1) {
    tabSet.openOrFocusTab({
      kind: "sessionChat",
      payload: {
        relatedFilesystemPath: null,
        sessionId: `session-${index}`,
      },
    } as any)
  }

  tabSet.activateTab("session:session-1")
  tabSet.activateTab("session:session-3")
  tabSet.closeTab("session:session-3")

  expect(tabSet.activeTabId).toBe("session:session-1")
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
      payload: {
        relatedFilesystemPath: null,
        sessionId: `session-${index}`,
      },
    } as any)
  }

  expect(closedTabIds).toEqual(["session:session-0"])
  expect(tabSet.tabs["session:session-0"]).toBeUndefined()
})
