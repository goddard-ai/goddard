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
      projectPath: null,
      sessionId: "session-1",
    },
  } as any)
  tabSet.closeTab("session:session-1")

  expect(closedTabIds).toEqual(["session:session-1"])
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
        projectPath: null,
        sessionId: `session-${index}`,
      },
    } as any)
  }

  expect(closedTabIds).toEqual(["session:session-0"])
  expect(tabSet.tabs["session:session-0"]).toBeUndefined()
})
