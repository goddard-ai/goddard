import { expect, test } from "bun:test"

import {
  createRestoredAppModels,
  observeAppStateSnapshot,
  type PersistedAppStateSnapshot,
} from "./app-state-persistence.ts"
import { Navigation } from "./navigation.ts"
import { shortcutRegistry } from "./shortcuts/shortcut-registry.ts"

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

test("app state persistence observes captured navigation snapshots", async () => {
  ensureMatchMedia()
  const appModels = createRestoredAppModels()
  const snapshots: PersistedAppStateSnapshot[] = []
  const observer = observeAppStateSnapshot(
    appModels,
    async (snapshot) => {
      snapshots.push(snapshot)
    },
    {
      debounceMs: 0,
    },
  )

  try {
    appModels.navigation.selectNavItem("sessions")
    await observer.flush()

    expect(snapshots).toHaveLength(1)
    expect(snapshots[0].navigation).toEqual({
      selectedNavId: "sessions",
    })
  } finally {
    await observer.stop()
  }
})

test("app state persistence does not observe shortcut registry changes", async () => {
  ensureMatchMedia()
  const appModels = createRestoredAppModels()
  const snapshots: PersistedAppStateSnapshot[] = []
  const observer = observeAppStateSnapshot(
    appModels,
    async (snapshot) => {
      snapshots.push(snapshot)
    },
    {
      debounceMs: 0,
    },
  )

  try {
    shortcutRegistry.applyKeymapSnapshot("goddard", {
      "navigation.openKeyboardShortcuts": ["Mod+/"],
    })
    await observer.flush()

    expect(snapshots).toHaveLength(0)
  } finally {
    shortcutRegistry.applyKeymapSnapshot("goddard", {})
    await observer.stop()
  }
})
