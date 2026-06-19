import { expect, test } from "vitest"

import { WorkbenchTabCache } from "./workbench-tab-cache.ts"

test("WorkbenchTabCache reuses values for the same tab key", () => {
  const cache = new WorkbenchTabCache()
  const first = cache.getOrCreate("tab-1", "chat", () => ({ value: { count: 1 } }))
  const second = cache.getOrCreate("tab-1", "chat", () => ({ value: { count: 2 } }))

  expect(second).toBe(first)
  expect(second.count).toBe(1)
})

test("WorkbenchTabCache starts setup once and disposes tab entries", () => {
  const cache = new WorkbenchTabCache()
  let setupCount = 0
  let setupCleanupCount = 0
  let disposeCount = 0

  cache.getOrCreate("tab-1", "chat", () => ({
    dispose: () => {
      disposeCount += 1
    },
    setup: () => {
      setupCount += 1
      return () => {
        setupCleanupCount += 1
      }
    },
    value: { count: 1 },
  }))

  cache.setup("tab-1", "chat")
  cache.setup("tab-1", "chat")
  cache.disposeTab("tab-1")
  cache.disposeTab("tab-1")

  expect(setupCount).toBe(1)
  expect(setupCleanupCount).toBe(1)
  expect(disposeCount).toBe(1)
})

test("WorkbenchTabCache only disposes entries owned by the requested tab", () => {
  const cache = new WorkbenchTabCache()
  let disposedTab1 = false
  let disposedTab2 = false

  cache.getOrCreate("tab-1", "chat", () => ({
    dispose: () => {
      disposedTab1 = true
    },
    value: {},
  }))
  cache.getOrCreate("tab-2", "chat", () => ({
    dispose: () => {
      disposedTab2 = true
    },
    value: {},
  }))

  cache.disposeTab("tab-1")

  expect(disposedTab1).toBe(true)
  expect(disposedTab2).toBe(false)
})
