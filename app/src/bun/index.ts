import { BrowserWindow, Screen, Updater } from "electrobun/bun"

import { activateDefaultLocale } from "~/language/i18n.ts"
import { loadAppStateSnapshot } from "./app-state-store.ts"
import { ensureDaemonRuntime } from "./daemon-runtime.ts"
import { installAppFatalErrorCapture, installAppLogCapture, writeAppError } from "./logging.ts"
import { getMainWindow, setMainWindow, showMainWindow } from "./main-window.ts"
import { installApplicationMenu } from "./menu.ts"
import { appRpc } from "./rpc.ts"
import {
  readWindowLayoutSnapshot,
  resolveInitialWindowFrame,
  writeMainWindowFrame,
  writeMainWindowFrameSync,
  type WindowFrame,
} from "./window-layout.ts"

const DEV_SERVER_PORT = 5173
const DEV_SERVER_URL = `http://127.0.0.1:${DEV_SERVER_PORT}`
const MAIN_WINDOW_READY_FALLBACK_MS = 5000

/** Creates the one primary Electrobun window used by the current app shell. */
function createMainWindow(url: string, frame: WindowFrame) {
  const window = new BrowserWindow({
    title: "Goddard",
    titleBarStyle: "hiddenInset",
    frame,
    url,
    hidden: true,
    rpc: appRpc,
    // Dev mode falls back to the native renderer when build.json is absent, so
    // opt into CEF here instead of relying on packaged build defaults.
    renderer: "cef",
    styleMask: {
      FullSizeContentView: false,
    },
  })

  installWindowLayoutPersistence(window)
  return window
}

function installMainWindowReadyFallback() {
  setTimeout(() => {
    if (showMainWindow()) {
      console.error("Renderer did not signal readiness before the main window fallback expired.")
    }
  }, MAIN_WINDOW_READY_FALLBACK_MS)
}

/** Returns the frontend URL, preferring the Vite dev server while Electrobun runs in dev mode. */
async function getMainWindowUrl() {
  const channel = await Updater.localInfo.channel()

  if (channel === "dev") {
    try {
      await fetch(DEV_SERVER_URL, { method: "HEAD" })
      console.log(`HMR enabled: using Vite dev server at ${DEV_SERVER_URL}`)
      return DEV_SERVER_URL
    } catch {
      console.log("Vite dev server not running. Run `pnpm run dev` to start the app with Vite.")
    }
  }

  return "views://main/index.html"
}

function installWindowLayoutPersistence(window: BrowserWindow<typeof appRpc>) {
  let saveTimer: ReturnType<typeof setTimeout> | null = null

  function clearSaveTimer() {
    if (saveTimer !== null) {
      clearTimeout(saveTimer)
      saveTimer = null
    }
  }

  function saveWindowFrame() {
    clearSaveTimer()

    if (window.isMinimized() || window.isFullScreen()) {
      return
    }

    void writeMainWindowFrame(window.getFrame()).catch((error) => {
      console.error("Failed to save window layout.", error)
    })
  }

  function saveWindowFrameBeforeClose() {
    clearSaveTimer()

    if (window.isMinimized() || window.isFullScreen()) {
      return
    }

    try {
      writeMainWindowFrameSync(window.getFrame())
    } catch (error) {
      console.error("Failed to save window layout.", error)
    }
  }

  function queueWindowFrameSave() {
    clearSaveTimer()
    saveTimer = setTimeout(saveWindowFrame, 500)
  }

  window.on("move", queueWindowFrameSave)
  window.on("resize", queueWindowFrameSave)
  window.on("close", saveWindowFrameBeforeClose)
}

installAppLogCapture()
installAppFatalErrorCapture()

async function main() {
  activateDefaultLocale()
  installApplicationMenu(getMainWindow)

  await ensureDaemonRuntime()
  const mainWindowUrl = await getMainWindowUrl()
  const windowLayout = readWindowLayoutSnapshot(await loadAppStateSnapshot())
  const mainWindowFrame = resolveInitialWindowFrame(
    windowLayout?.mainWindow.frame ?? null,
    Screen.getAllDisplays(),
    Screen.getPrimaryDisplay(),
  )
  const mainWindow = createMainWindow(mainWindowUrl, mainWindowFrame)
  setMainWindow(mainWindow)
  installMainWindowReadyFallback()
}

await main().catch((error) => {
  writeAppError("app.host.startup_failed", error)
  process.exit(1)
})
