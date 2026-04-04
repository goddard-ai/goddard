import { BrowserWindow, Updater } from "electrobun/bun"
import { getMainWindow, setMainWindow } from "./main-window"
import { installMacOsApplicationMenu } from "./menu"
import { appRpc } from "./rpc"

const DEV_SERVER_PORT = 5173
const DEV_SERVER_URL = `http://127.0.0.1:${DEV_SERVER_PORT}`

/** Creates the one primary Electrobun window used by the current app shell. */
function createMainWindow(url: string) {
  const window = new BrowserWindow({
    title: "Goddard",
    titleBarStyle: "hiddenInset",
    url,
    rpc: appRpc,
    styleMask: {
      FullSizeContentView: false,
    },
  })

  window.show()
  return window
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
      console.log("Vite dev server not running. Run `bun run dev` to start the app with Vite.")
    }
  }

  return "views://main/index.html"
}

installMacOsApplicationMenu(getMainWindow)

const mainWindowUrl = await getMainWindowUrl()
const mainWindow = createMainWindow(mainWindowUrl)
setMainWindow(mainWindow)
