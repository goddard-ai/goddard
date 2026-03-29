import { BrowserView } from "electrobun/bun"
import type { AppDesktopRpc } from "../shared/desktop-rpc"
import { daemonSend } from "./daemon"

/** Shared Bun-side Electrobun RPC handlers for the desktop app. */
export const appRpc = BrowserView.defineRPC<AppDesktopRpc>({
  handlers: {
    requests: {
      runtimeInfo: async () => ({ runtime: "electrobun" }),
      daemonSend: async (input) => await daemonSend(input),
    },
    messages: {},
  },
})
