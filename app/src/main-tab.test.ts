import { expect, test } from "bun:test"

import { MainTab } from "./main-tab.ts"

test("main tab lists primary workbench items", () => {
  const mainTab = new MainTab()

  expect(mainTab.items.map((item) => item.id)).toEqual([
    "inbox",
    "sessions",
    "pipelines",
    "search",
    "specs",
    "tasks",
    "roadmap",
  ])
})
