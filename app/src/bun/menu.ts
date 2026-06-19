import { ApplicationMenu, ApplicationMenuItemConfig, type BrowserWindow } from "electrobun/bun"
import { concat } from "radashi"

import { i18n } from "~/language/i18n.ts"
import type { AppCommandId } from "~/shared/app-commands.ts"
import { DebugMenuSurfaces, type DebugMenuSurface } from "~/shared/debug-menu.ts"
import { dispatchGlobalEvent } from "./rpc.ts"

function menuText(message: string) {
  return i18n._(message)
}

const fileMenu = {
  get label() {
    return menuText("File")
  },
  closeWindow: {
    get label() {
      return menuText("Close Window")
    },
    action: "file:close-window",
    accelerator: "CommandOrControl+Shift+W",
  },
  closeTab: {
    get label() {
      return menuText("Close Tab")
    },
    action: "file:close-tab",
    accelerator: "CommandOrControl+W",
  },
} as const

const viewMenu = {
  get label() {
    return menuText("View")
  },
  commandPalette: {
    get label() {
      return menuText("Command Palette")
    },
    action: "view:command-palette",
  },
  reload: {
    get label() {
      return menuText("Reload")
    },
    action: "view:reload",
    accelerator: "CommandOrControl+R",
  },
  inspectElement: {
    get label() {
      return menuText("Inspect Element")
    },
    action: "view:inspect-element",
    accelerator: "Alt+CommandOrControl+I",
  },
} as const

/** Installs the native application menu so platform accelerators work inside the desktop shell. */
export function installApplicationMenu(getMainWindow: () => BrowserWindow | null): void {
  const actions: Record<string, (window: BrowserWindow, params: any) => void> = {
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
      label: fileMenu.label,
      submenu: concat(
        fileMenu.closeTab,
        fileMenu.closeWindow,
        process.platform === "darwin"
          ? [{ type: "separator" as const }, { role: "quit", accelerator: "CommandOrControl+Q" }]
          : null,
      ),
    },
    {
      label: menuText("Edit"),
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
          label: menuText("Developer"),
          submenu: [viewMenu.inspectElement],
        },
      ],
    },
  ]

  if (debugMenu.length > 0) {
    menu.push({
      label: menuText("Debug"),
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
      actions[action]?.(mainWindow, JSON.parse(params))
    }
  })
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
