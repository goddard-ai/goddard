import { expect, test } from "bun:test"

import { restoreNavigationState } from "./app-state-persistence.ts"
import { Navigation } from "./navigation.ts"

const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v3"

test("navigation omits projects from the primary workbench items", () => {
  window.localStorage.clear()

  const navigation = new Navigation()

  expect(navigation.items.map((item) => item.id)).toEqual([
    "inbox",
    "sessions",
    "search",
    "specs",
    "tasks",
    "roadmap",
  ])
})

test("navigation persistence ignores removed navigation ids", () => {
  window.localStorage.clear()
  window.localStorage.setItem(
    NAVIGATION_STORAGE_KEY,
    JSON.stringify({
      version: 1,
      savedAt: 100,
      value: { selectedNavId: "projects" },
    }),
  )

  const navigation = new Navigation()

  restoreNavigationState(navigation)

  expect(navigation.selectedNavId).toBe("inbox")
})
