import { expect, test } from "bun:test"

import {
  createAppState,
  observeAppStateSnapshot,
  type AppStateSnapshot,
} from "./app-state-persistence.ts"
import { MainTab } from "./main-tab.ts"
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

test("app state persistence observes captured main tab snapshots", async () => {
  ensureMatchMedia()
  const appState = createAppState()
  const snapshots: AppStateSnapshot[] = []
  const observer = observeAppStateSnapshot(
    appState,
    async (snapshot) => {
      snapshots.push(snapshot)
    },
    {
      debounceMs: 0,
    },
  )

  try {
    appState.mainTab.selectKind("sessions")
    await observer.flush()

    expect(snapshots).toHaveLength(1)
    expect(snapshots[0].mainTab).toEqual({
      selectedKind: "sessions",
    })
  } finally {
    await observer.stop()
  }
})

test("app state persistence does not observe shortcut registry changes", async () => {
  ensureMatchMedia()
  const appState = createAppState()
  const snapshots: AppStateSnapshot[] = []
  const observer = observeAppStateSnapshot(
    appState,
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
