import { BrowserView } from "electrobun/bun"
import type { AppDesktopRpc } from "../shared/desktop-rpc"
import { daemonSend } from "./daemon"
import { browseForProject, inspectProject } from "./projects"

/** Shared Bun-side Electrobun RPC handlers for the desktop app. */
export const appRpc = BrowserView.defineRPC<AppDesktopRpc>({
  handlers: {
    requests: {
      runtimeInfo: async () => ({ runtime: "electrobun" }),
      browseForProject: async () => ({ path: await browseForProject() }),
      inspectProject: async ({ path }) => await inspectProject(path),
      daemonSend: async (input) => await daemonSend(input),
    },
    messages: {},
  },
})
