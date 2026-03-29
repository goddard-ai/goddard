import { BrowserWindow } from "electrobun/bun"
import { installMacOsApplicationMenu } from "./menu"
import { appRpc } from "./rpc"

/** Creates the one primary Electrobun window used by the current app shell. */
function createMainWindow(): BrowserWindow<typeof appRpc> {
  const window = new BrowserWindow({
    title: "Goddard",
    url: "views://main/index.html",
    rpc: appRpc,
  })

  window.show()
  return window
}

installMacOsApplicationMenu()
createMainWindow()
