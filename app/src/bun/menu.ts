import { ApplicationMenu, ApplicationMenuItemConfig, type BrowserWindow } from "electrobun/bun"
import { createDebugMenuDispatchScript, DebugMenuSurface } from "../shared/debug-menu"

const withParams =
  <T>(action: string) =>
  (params: T) =>
    `${action}/${JSON.stringify(params)}`

const reloadViewAction = "view:reload"
const inspectElementAction = "view:inspect-element"
const debugNavigateAction = withParams<DebugMenuSurface>("debug:navigate")

function reloadWindow(window: BrowserWindow): void {
  window.webview.executeJavascript("window.location.reload()")
}

function inspectWindow(window: BrowserWindow): void {
  window.webview.openDevTools()
}

/** Dispatches one development-menu surface request into the active webview. */
function openDebugSurface(surface: DebugMenuSurface) {
  return (window: BrowserWindow): void => {
    window.webview.executeJavascript(createDebugMenuDispatchScript({ surface }))
  }
}

/** Installs the standard macOS Edit menu so native text shortcuts work in webviews. */
export function installMacOsApplicationMenu(getMainWindow: () => BrowserWindow | null): void {
  if (process.platform !== "darwin") {
    return
  }

  const actions: Record<string, (window: BrowserWindow, params: any) => void> = {
    [reloadViewAction]: reloadWindow,
    [inspectElementAction]: inspectWindow,
  }

  const debugMenu: ApplicationMenuItemConfig[] = []
  if (Bun.env.NODE_ENV === "development") {
    const surfaces: { label: string; surface: DebugMenuSurface }[] = [
      { label: "Terminal", surface: "terminal" },
    ]
    debugMenu.push(
      ...surfaces.map(({ label, surface }): ApplicationMenuItemConfig => {
        const action = debugNavigateAction(surface)
        actions[action] = openDebugSurface(surface)
        return {
          label,
          action,
        }
      }),
    )
  }

  const menu: ApplicationMenuItemConfig[] = [
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
