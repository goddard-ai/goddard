import type { Display } from "electrobun/bun"

import {
  WindowLayoutSnapshot as WindowLayoutSnapshotSchema,
  type AppStateSnapshot,
  type WindowFrame,
  type WindowLayoutSnapshot,
} from "~/shared/app-state.ts"
import { updateAppStateSnapshot, updateAppStateSnapshotSync } from "./app-state-store.ts"

export type { WindowFrame } from "~/shared/app-state.ts"

const MIN_RESTORED_WINDOW_WIDTH = 320
const MIN_RESTORED_WINDOW_HEIGHT = 240
const MIN_VISIBLE_WINDOW_SIZE = 80
const DEFAULT_WINDOW_FRAME: WindowFrame = {
  x: 0,
  y: 0,
  width: 1440,
  height: 900,
}

type DisplayFrame = Display["workArea"]

export function readWindowLayoutSnapshot(
  snapshot: AppStateSnapshot | null,
): WindowLayoutSnapshot | null {
  const result = WindowLayoutSnapshotSchema.safeParse(snapshot?.windowLayout)
  return result.success ? result.data : null
}

export function resolveInitialWindowFrame(
  savedFrame: WindowFrame | null,
  displays: readonly Display[],
  primaryDisplay: Display,
): WindowFrame {
  if (savedFrame && canRestoreFrame(savedFrame, displays)) {
    return savedFrame
  }

  return getDisplayFrame(primaryDisplay) ?? DEFAULT_WINDOW_FRAME
}

export async function writeMainWindowFrame(frame: WindowFrame) {
  await updateAppStateSnapshot((snapshot) => ({
    ...snapshot,
    windowLayout: {
      mainWindow: {
        frame,
      },
    },
  }))
}

export function writeMainWindowFrameSync(frame: WindowFrame) {
  updateAppStateSnapshotSync((snapshot) => ({
    ...snapshot,
    windowLayout: {
      mainWindow: {
        frame,
      },
    },
  }))
}

function canRestoreFrame(frame: WindowFrame, displays: readonly Display[]) {
  if (frame.width < MIN_RESTORED_WINDOW_WIDTH || frame.height < MIN_RESTORED_WINDOW_HEIGHT) {
    return false
  }

  return displays.some((display) => {
    const displayFrame = getDisplayFrame(display)
    return displayFrame ? overlapsEnough(frame, displayFrame) : false
  })
}

function getDisplayFrame(display: Display): DisplayFrame | null {
  if (display.workArea.width > 0 && display.workArea.height > 0) {
    return display.workArea
  }

  if (display.bounds.width > 0 && display.bounds.height > 0) {
    return display.bounds
  }

  return null
}

function overlapsEnough(frame: WindowFrame, displayFrame: DisplayFrame) {
  const left = Math.max(frame.x, displayFrame.x)
  const top = Math.max(frame.y, displayFrame.y)
  const right = Math.min(frame.x + frame.width, displayFrame.x + displayFrame.width)
  const bottom = Math.min(frame.y + frame.height, displayFrame.y + displayFrame.height)

  return right - left >= MIN_VISIBLE_WINDOW_SIZE && bottom - top >= MIN_VISIBLE_WINDOW_SIZE
}
