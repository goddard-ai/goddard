import { BrowserView } from "electrobun/bun"
import type { AppDesktopRpc } from "~/shared/desktop-rpc.ts"
import { daemonSend } from "./daemon.ts"
import { getMainWindow } from "./main-window.ts"
import { browseForProject } from "./projects.ts"

type AppRpc = ReturnType<typeof BrowserView.defineRPC<AppDesktopRpc>>

/** Shared Bun-side Electrobun RPC handlers for the desktop app. */
export const appRpc: AppRpc = BrowserView.defineRPC<AppDesktopRpc>({
  handlers: {
    requests: {
      runtimeInfo: async () => ({ runtime: "electrobun" }),
      browseForProject: async () => ({ path: await browseForProject() }),
      daemonSend: async (input) => await daemonSend(input),
      maximizeWindow: async () => {
        getMainWindow()?.maximize()
        return {}
      },
    },
    messages: {},
  },
})
