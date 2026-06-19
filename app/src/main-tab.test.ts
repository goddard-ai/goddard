import { expect, test } from "vitest"

import { MainTab } from "./main-tab.ts"

test("main tab omits projects from the primary workbench items", () => {
  const mainTab = new MainTab()

  expect(mainTab.items.map((item) => item.id)).toEqual([
    "inbox",
    "sessions",
    "search",
    "specs",
    "tasks",
    "roadmap",
  ])
})
