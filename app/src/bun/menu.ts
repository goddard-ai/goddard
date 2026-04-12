import { ApplicationMenu, ApplicationMenuItemConfig, type BrowserWindow } from "electrobun/bun"
import { concat } from "radashi"

import type { AppMenuAction } from "~/shared/app-menu.ts"
import { DebugMenuSurfaces, type DebugMenuSurface } from "~/shared/debug-menu.ts"
import { ShortcutCommands } from "~/shared/shortcut-keymap.ts"
import { dispatchGlobalEvent } from "./rpc.ts"

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

const debugNavigateAction = "debug:navigate"

/** Installs the native application menu so platform accelerators work inside the desktop shell. */
export function installApplicationMenu(getMainWindow: () => BrowserWindow | null): void {
  const actions: Record<string, (window: BrowserWindow, params: unknown) => void> = {
    [fileMenu.closeTab.action]: dispatchAppMenuAction(ShortcutCommands.closeActiveTab),
    [fileMenu.closeWindow.action]: closeWindow,
    [viewMenu.reload.action]: reloadWindow,
    [viewMenu.inspectElement.action]: inspectWindow,
  }

  const debugMenu: ApplicationMenuItemConfig[] = []
  if (isDevelopmentRuntime()) {
    const debugNavigate = withParams<DebugMenuSurface>(debugNavigateAction)

    for (const surface of Object.values(DebugMenuSurfaces)) {
      const action = debugNavigate(surface)
      actions[action] = dispatchDebugMenuAction
      debugMenu.push({ label: surface, action })
    }
  }

  const menu: ApplicationMenuItemConfig[] = [
    {
      label: fileMenu.label,
      submenu: concat(
        fileMenu.closeTab,
        fileMenu.closeWindow,
        process.platform === "darwin" ? [{ type: "separator" as const }, { role: "quit" }] : null,
      ),
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
function dispatchAppMenuAction(action: AppMenuAction) {
  return (_window: BrowserWindow): void => {
    dispatchGlobalEvent({
      name: "appMenu",
      detail: { action },
    })
  }
}

/** Closes the current native window. */
function closeWindow(window: BrowserWindow): void {
  window.close()
}

/** Dispatches one development-menu surface request into the active webview. */
function dispatchDebugMenuAction(_window: BrowserWindow, params: unknown): void {
  dispatchGlobalEvent({
    name: "debugMenu",
    detail: { surface: params as DebugMenuSurface },
  })
}

/** Creates a function that dispatches one action with one parameter. */
function withParams<T>(action: string) {
  return (params: T) => `${action}/${JSON.stringify(params)}`
}

/** Returns whether the current Bun runtime should expose development-only menu items. */
function isDevelopmentRuntime(): boolean {
  return (
    process.env.NODE_ENV === "development" ||
    Bun.env.NODE_ENV === "development" ||
    Bun.argv.some((argument) => argument === "--watch" || argument === "dev")
  )
}
