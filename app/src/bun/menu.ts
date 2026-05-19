import { ApplicationMenu, ApplicationMenuItemConfig, type BrowserWindow } from "electrobun/bun"

import type { AppCommandId } from "~/shared/app-commands.ts"
import { DebugMenuSurfaces, type DebugMenuSurface } from "~/shared/debug-menu.ts"
import { dispatchGlobalEvent } from "./rpc.ts"
import { applyReadyUpdate, checkAndDownloadUpdate } from "./updater.ts"

const fileMenu = {
  label: "File",
  closeWindow: {
    label: "Close Window",
    action: "file:close-window",
    accelerator: "CommandOrControl+Shift+W",
  },
  closeTab: {
    label: "Close Tab",
    action: "file:close-tab",
    accelerator: "CommandOrControl+W",
  },
} as const

const viewMenu = {
  label: "View",
  commandPalette: {
    label: "Command Palette",
    action: "view:command-palette",
  },
  reload: {
    label: "Reload",
    action: "view:reload",
    accelerator: "CommandOrControl+R",
  },
  inspectElement: {
    label: "Inspect Element",
    action: "view:inspect-element",
    accelerator: "Alt+CommandOrControl+I",
  },
} as const

const appUpdateMenu = {
  label: "Goddard",
  checkForUpdates: {
    label: "Check for Updates...",
    action: "app:check-for-updates",
  },
  restartToUpdate: {
    label: "Restart to Update",
    action: "app:restart-to-update",
  },
} as const

/** Installs the native application menu so platform accelerators work inside the desktop shell. */
export function installApplicationMenu(getMainWindow: () => BrowserWindow | null): void {
  const actions: Record<string, (window: BrowserWindow, params: any) => Promise<void> | void> = {
    [appUpdateMenu.checkForUpdates.action]: checkForUpdates,
    [appUpdateMenu.restartToUpdate.action]: restartToUpdate,
    [fileMenu.closeTab.action]: dispatchAppMenuAction("workbench.closeActiveTab"),
    [fileMenu.closeWindow.action]: closeWindow,
    [viewMenu.commandPalette.action]: dispatchAppMenuAction("navigation.openCommandPalette"),
    [viewMenu.reload.action]: reloadWindow,
    [viewMenu.inspectElement.action]: inspectWindow,
  }

  const debugMenu: ApplicationMenuItemConfig[] = []
  if (isDevelopmentRuntime()) {
    for (const surface of Object.values(DebugMenuSurfaces)) {
      const action = `debug:${surface}`
      actions[action] = dispatchDebugMenuAction(surface)
      debugMenu.push({ label: surface, action })
    }
  }

  const menu: ApplicationMenuItemConfig[] = [
    {
      label: appUpdateMenu.label,
      submenu: [
        appUpdateMenu.checkForUpdates,
        appUpdateMenu.restartToUpdate,
        ...(process.platform === "darwin"
          ? [{ type: "separator" as const }, { role: "quit" }]
          : []),
      ],
    },
    {
      label: fileMenu.label,
      submenu: [fileMenu.closeTab, fileMenu.closeWindow],
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
      label: viewMenu.label,
      submenu: [
        viewMenu.commandPalette,
        viewMenu.reload,
        {
          label: "Developer",
          submenu: [viewMenu.inspectElement],
        },
      ],
    },
  ]

  if (debugMenu.length > 0) {
    menu.push({
      label: "Debug",
      submenu: debugMenu,
    })
  }

  ApplicationMenu.setApplicationMenu(menu)

  ApplicationMenu.on("application-menu-clicked", (event: any) => {
    const [action, params = "null"] = event.data?.action?.split("/") ?? []
    if (!action) {
      return
    }
    const mainWindow = getMainWindow()
    if (mainWindow) {
      void Promise.resolve(actions[action]?.(mainWindow, JSON.parse(params))).catch((error) => {
        console.error(`Application menu action failed: ${action}.`, error)
      })
    }
  })
}

/** Checks for a packaged app update from the native application menu. */
async function checkForUpdates(_window: BrowserWindow) {
  const result = await checkAndDownloadUpdate()
  console.log(`Update check finished: ${result}.`)
}

/** Applies a downloaded app update from the native application menu when one is ready. */
async function restartToUpdate(_window: BrowserWindow) {
  const result = await applyReadyUpdate()

  if (result !== "ready") {
    console.log(`No ready update to apply: ${result}.`)
  }
}

function reloadWindow(window: BrowserWindow): void {
  window.webview.executeJavascript("window.location.reload()")
}

function inspectWindow(window: BrowserWindow): void {
  window.webview.openDevTools()
}

/** Dispatches one app menu action into the active webview. */
function dispatchAppMenuAction(command: AppCommandId) {
  return (_window: BrowserWindow): void => {
    dispatchGlobalEvent({
      name: "appMenu",
      detail: { command },
    })
  }
}

/** Closes the current native window. */
function closeWindow(window: BrowserWindow): void {
  window.close()
}

/** Dispatches one development-menu surface request into the active webview. */
function dispatchDebugMenuAction(surface: DebugMenuSurface) {
  return (_window: BrowserWindow): void => {
    dispatchGlobalEvent({
      name: "debugMenu",
      detail: { surface },
    })
  }
}

/** Returns whether the current Bun runtime should expose development-only menu items. */
function isDevelopmentRuntime(): boolean {
  return (
    process.env.NODE_ENV === "development" ||
    Bun.env.NODE_ENV === "development" ||
    Bun.argv.some((argument) => argument === "--watch" || argument === "dev")
  )
}
