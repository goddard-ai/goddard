import { expect, test } from "bun:test"

import { Navigation } from "./navigation.ts"

const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v2"

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

test("hydrateNavigation ignores removed persisted navigation ids", () => {
  window.localStorage.clear()
  window.localStorage.setItem(NAVIGATION_STORAGE_KEY, JSON.stringify({ selectedNavId: "projects" }))

  const navigation = new Navigation()

  navigation.hydrateNavigation()

  expect(navigation.selectedNavId).toBe("inbox")
})
