import { ApplicationMenu, type BrowserWindow } from "electrobun/bun"

const reloadViewAction = "view:reload"
const inspectElementAction = "view:inspect-element"

function reloadWindow(window: BrowserWindow): void {
  window.webview.executeJavascript("window.location.reload()")
}

function inspectWindow(window: BrowserWindow): void {
  window.webview.openDevTools()
}

/** Installs the standard macOS Edit menu so native text shortcuts work in webviews. */
export function installMacOsApplicationMenu(getMainWindow: () => BrowserWindow | null): void {
  if (process.platform !== "darwin") {
    return
  }

  ApplicationMenu.on("application-menu-clicked", (event) => {
    const menuEvent = event as { data?: { action?: string } }
    const mainWindow = getMainWindow()

    if (!mainWindow) {
      return
    }

    if (menuEvent.data?.action === reloadViewAction) {
      reloadWindow(mainWindow)
      return
    }

    if (menuEvent.data?.action === inspectElementAction) {
      inspectWindow(mainWindow)
    }
  })

  ApplicationMenu.setApplicationMenu([
    {
      submenu: [{ label: "Quit", role: "quit", accelerator: "cmd+q" }],
    },
    {
      label: "Edit",
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "pasteAndMatchStyle" },
        { role: "delete" },
        { role: "selectAll" },
      ],
    },
    {
      label: "View",
      submenu: [
        {
          label: "Reload",
          action: reloadViewAction,
          accelerator: "cmd+r",
        },
        {
          label: "Developer",
          submenu: [
            {
              label: "Inspect Element",
              action: inspectElementAction,
              accelerator: "cmd+alt+i",
            },
          ],
        },
      ],
    },
  ])
}
