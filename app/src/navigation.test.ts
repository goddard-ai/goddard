import { expect, test } from "bun:test"

import { createRestoredAppState } from "./app-state-persistence.ts"
import { Navigation } from "./navigation.ts"

const NAVIGATION_STORAGE_KEY = "goddard.app.navigation.v3"

function ensureMatchMedia() {
  window.matchMedia = (() => {
    return {
      matches: false,
      media: "(prefers-color-scheme: dark)",
      onchange: null,
      addListener() {},
      removeListener() {},
      addEventListener() {},
      removeEventListener() {},
      dispatchEvent() {
        return false
      },
    }
  }) as typeof window.matchMedia
}

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

test("navigation persistence restores selected navigation id", () => {
  window.localStorage.clear()
  window.localStorage.setItem(
    NAVIGATION_STORAGE_KEY,
    JSON.stringify({
      version: 1,
      savedAt: 100,
      value: { selectedNavId: "sessions" },
    }),
  )

  ensureMatchMedia()
  const appState = createRestoredAppState({
    mode: "system",
    highContrast: false,
  })

  expect(appState.navigation.selectedNavId).toBe("sessions")
})
