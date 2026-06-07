import { expect, test } from "bun:test"
import type { Display } from "electrobun/bun"

import type { AppStateSnapshot, WindowFrame } from "~/shared/app-state.ts"
import { readWindowLayoutSnapshot, resolveInitialWindowFrame } from "./window-layout.ts"

test("window layout reads the persisted main-window frame", () => {
  const snapshot: AppStateSnapshot = {
    windowLayout: {
      mainWindow: {
        frame: {
          x: 40,
          y: 60,
          width: 1200,
          height: 800,
        },
      },
    },
  }

  expect(readWindowLayoutSnapshot(snapshot)?.mainWindow.frame).toEqual({
    x: 40,
    y: 60,
    width: 1200,
    height: 800,
  })
})

test("window layout ignores invalid persisted layout shapes", () => {
  expect(
    readWindowLayoutSnapshot({
      windowLayout: {
        mainWindow: {
          frame: {
            x: 40,
            y: 60,
            width: -1,
            height: 800,
          },
        },
      },
    }),
  ).toBeNull()
})

test("window layout restores saved frames visible on a current display", () => {
  const savedFrame: WindowFrame = {
    x: 200,
    y: 160,
    width: 1100,
    height: 760,
  }

  expect(resolveInitialWindowFrame(savedFrame, [createDisplay()], createDisplay())).toBe(savedFrame)
})

test("window layout fills the primary work area when no saved layout exists", () => {
  expect(resolveInitialWindowFrame(null, [createDisplay()], createDisplay())).toEqual({
    x: 0,
    y: 25,
    width: 1440,
    height: 875,
  })
})

test("window layout ignores saved frames that are no longer visible", () => {
  const savedFrame: WindowFrame = {
    x: 5000,
    y: 5000,
    width: 1100,
    height: 760,
  }

  expect(resolveInitialWindowFrame(savedFrame, [createDisplay()], createDisplay())).toEqual({
    x: 0,
    y: 25,
    width: 1440,
    height: 875,
  })
})

function createDisplay(): Display {
  return {
    id: 1,
    bounds: {
      x: 0,
      y: 0,
      width: 1440,
      height: 900,
    },
    workArea: {
      x: 0,
      y: 25,
      width: 1440,
      height: 875,
    },
    scaleFactor: 1,
    isPrimary: true,
  }
}
