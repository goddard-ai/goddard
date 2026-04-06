import { BrowserView } from "electrobun/bun"
import type { AppDesktopRpc } from "~/shared/desktop-rpc"
import { daemonSend } from "./daemon"
import { browseForProject } from "./projects"

type AppRpc = ReturnType<typeof BrowserView.defineRPC<AppDesktopRpc>>

/** Shared Bun-side Electrobun RPC handlers for the desktop app. */
export const appRpc: AppRpc = BrowserView.defineRPC<AppDesktopRpc>({
  handlers: {
    requests: {
      runtimeInfo: async () => ({ runtime: "electrobun" }),
      browseForProject: async () => ({ path: await browseForProject() }),
      daemonSend: async (input) => await daemonSend(input),
    },
    messages: {},
  },
})
