import { ApplicationMenu, ApplicationMenuItemConfig, type BrowserWindow } from "electrobun/bun"
import { createAppMenuDispatchScript, type AppMenuAction } from "../shared/app-menu"
import { createDebugMenuDispatchScript, type DebugMenuSurface } from "../shared/debug-menu"

const withParams =
  <T>(action: string) =>
  (params: T) =>
    `${action}/${JSON.stringify(params)}`

const reloadViewAction = "view:reload"
const inspectElementAction = "view:inspect-element"
const closeTabAction = "file:close-tab"
const closeWindowAction = "file:close-window"
const debugNavigateAction = withParams<DebugMenuSurface>("debug:navigate")

function reloadWindow(window: BrowserWindow): void {
  window.webview.executeJavascript("window.location.reload()")
}

function inspectWindow(window: BrowserWindow): void {
  window.webview.openDevTools()
}

/** Dispatches one app menu action into the active webview. */
function dispatchAppMenuAction(action: AppMenuAction) {
  return (window: BrowserWindow): void => {
    window.webview.executeJavascript(createAppMenuDispatchScript({ action }))
  }
}

/** Closes the current native window. */
function closeWindow(window: BrowserWindow): void {
  window.close()
}

/** Dispatches one development-menu surface request into the active webview. */
function openDebugSurface(surface: DebugMenuSurface) {
  return (window: BrowserWindow): void => {
    window.webview.executeJavascript(createDebugMenuDispatchScript({ surface }))
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

/** Installs the standard macOS Edit menu so native text shortcuts work in webviews. */
export function installMacOsApplicationMenu(getMainWindow: () => BrowserWindow | null): void {
  if (process.platform !== "darwin") {
    return
  }

  const actions: Record<string, (window: BrowserWindow, params: unknown) => void> = {
    [closeTabAction]: dispatchAppMenuAction("closeTab"),
    [closeWindowAction]: closeWindow,
    [reloadViewAction]: reloadWindow,
    [inspectElementAction]: inspectWindow,
  }
  const showDebugMenu = isDevelopmentRuntime()

  const debugMenu: ApplicationMenuItemConfig[] = []
  if (showDebugMenu) {
    const surfaces: { label: string; surface: DebugMenuSurface; accelerator?: string }[] = [
      { label: "Terminal", surface: "terminal" },
      {
        label: "SessionChatTranscript",
        surface: "sessionChatTranscript",
        accelerator: "cmd+alt+9",
      },
    ]
    debugMenu.push(
      ...surfaces.map(({ label, surface, accelerator }): ApplicationMenuItemConfig => {
        const action = debugNavigateAction(surface)
        actions[action] = openDebugSurface(surface)
        return {
          label,
          action,
          accelerator,
        }
      }),
    )
  }

  const menu: ApplicationMenuItemConfig[] = [
    {
      submenu: [{ label: "Quit", role: "quit", accelerator: "cmd+q" }],
    },
    {
      label: "File",
      submenu: [
        {
          label: "Close Tab",
          action: closeTabAction,
          accelerator: "cmd+w",
        },
        {
          label: "Close Window",
          action: closeWindowAction,
          accelerator: "cmd+shift+w",
        },
      ],
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
