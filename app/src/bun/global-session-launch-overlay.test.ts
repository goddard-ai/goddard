import { expect, test } from "bun:test"

import {
  createGlobalSessionLaunchOverlayHost,
  findActiveDisplay,
} from "./global-session-launch-overlay-core.ts"

const primaryDisplay = {
  id: 1,
  bounds: { x: 0, y: 0, width: 1280, height: 800 },
  workArea: { x: 0, y: 0, width: 1280, height: 760 },
  scaleFactor: 1,
  isPrimary: true,
}

const secondaryDisplay = {
  id: 2,
  bounds: { x: 1280, y: 0, width: 1440, height: 900 },
  workArea: { x: 1280, y: 0, width: 1440, height: 860 },
  scaleFactor: 2,
  isPrimary: false,
}

function createShortcutDriver() {
  const callbacks = new Map<string, () => void>()

  return {
    callbacks,
    driver: {
      isRegistered(accelerator: string) {
        return callbacks.has(accelerator)
      },
      register(accelerator: string, callback: () => void) {
        if (callbacks.has(accelerator) || accelerator === "Command+Conflict") {
          return false
        }

        callbacks.set(accelerator, callback)
        return true
      },
      unregister(accelerator: string) {
        return callbacks.delete(accelerator)
      },
    },
  }
}

function createUnusedOverlayDrivers() {
  return {
    createWindow() {
      throw new Error("Overlay window should not be created by this test.")
    },
    screen: {
      getAllDisplays: () => [primaryDisplay],
      getCursorScreenPoint: () => ({ x: 0, y: 0 }),
      getPrimaryDisplay: () => primaryDisplay,
    },
  }
}

test("findActiveDisplay resolves the display containing the cursor", () => {
  expect(
    findActiveDisplay(
      {
        getAllDisplays: () => [primaryDisplay, secondaryDisplay],
        getCursorScreenPoint: () => ({ x: 1400, y: 200 }),
        getPrimaryDisplay: () => primaryDisplay,
      },
    ),
  ).toEqual(secondaryDisplay)
})

test("findActiveDisplay falls back to the primary display", () => {
  expect(
    findActiveDisplay(
      {
        getAllDisplays: () => [secondaryDisplay],
        getCursorScreenPoint: () => ({ x: -100, y: -100 }),
        getPrimaryDisplay: () => primaryDisplay,
      },
    ),
  ).toEqual(primaryDisplay)
})

test("global launch shortcut registration reports unavailable bindings", () => {
  const shortcuts = createShortcutDriver()
  const host = createGlobalSessionLaunchOverlayHost(
    {
      getOverlayUrl: () => "views://main/index.html",
      onShortcut() {},
      rpc: {} as any,
    },
    {
      globalShortcut: shortcuts.driver,
      ...createUnusedOverlayDrivers(),
    },
  )

  expect(host.registerGlobalShortcut("Command+Conflict")).toEqual({
    registered: false,
    reason: "unavailable",
  })
  expect(shortcuts.callbacks.size).toBe(0)
})

test("registering a replacement shortcut unregisters the previous binding", () => {
  const shortcuts = createShortcutDriver()
  const host = createGlobalSessionLaunchOverlayHost(
    {
      getOverlayUrl: () => "views://main/index.html",
      onShortcut() {},
      rpc: {} as any,
    },
    {
      globalShortcut: shortcuts.driver,
      ...createUnusedOverlayDrivers(),
    },
  )

  expect(host.registerGlobalShortcut("Command+Period")).toEqual({ registered: true })
  expect(host.registerGlobalShortcut("Command+Space")).toEqual({ registered: true })
  expect(shortcuts.callbacks.has("Command+Period")).toBe(false)
  expect(shortcuts.callbacks.has("Command+Space")).toBe(true)
})

test("overlay show targets the active display and toggle hides the same window", () => {
  const calls: string[] = []
  const frames: Array<{ x: number; y: number; width: number; height: number }> = []
  const host = createGlobalSessionLaunchOverlayHost(
    {
      getOverlayUrl: () => "views://main/index.html",
      onShortcut() {},
      rpc: {} as any,
    },
    {
      createWindow(options) {
        frames.push(options.frame!)
        return {
          hide() {
            calls.push("hide")
          },
          setAlwaysOnTop(alwaysOnTop) {
            calls.push(`alwaysOnTop:${alwaysOnTop}`)
          },
          setFrame(x, y, width, height) {
            frames.push({ x, y, width, height })
          },
          show() {
            calls.push("show")
          },
        }
      },
      screen: {
        getAllDisplays: () => [primaryDisplay, secondaryDisplay],
        getCursorScreenPoint: () => ({ x: 1400, y: 200 }),
        getPrimaryDisplay: () => primaryDisplay,
      },
    },
  )

  host.toggleOverlay()
  host.toggleOverlay()

  expect(frames).toEqual([secondaryDisplay.bounds, secondaryDisplay.bounds])
  expect(calls).toEqual(["alwaysOnTop:true", "alwaysOnTop:true", "show", "hide"])
  expect(host.isOverlayVisible()).toBe(false)
})
