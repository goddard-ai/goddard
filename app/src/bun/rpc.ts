import { BrowserView, Utils } from "electrobun/bun"

import type { AppDesktopRpc } from "~/shared/desktop-rpc.ts"
import type { GlobalEventEnvelope } from "~/shared/global-event-hub.ts"
import { loadAppStateSnapshot, writeAppStateSnapshot } from "./app-state-store.ts"
import {
  daemonResetSubscriptions,
  daemonSend,
  daemonSubscribe,
  daemonUnsubscribe,
} from "./daemon.ts"
import { writeAppLog } from "./logging.ts"
import { getMainWindow } from "./main-window.ts"
import { getProjectGitStatus } from "./project-git-status.ts"
import { browseForProject } from "./projects.ts"
import { loadShortcutKeymap, writeShortcutKeymap } from "./shortcut-keymap.ts"
import { listWorktreeOpeners, openWorktree } from "./worktree-openers.ts"

type AppRpc = ReturnType<typeof BrowserView.defineRPC<AppDesktopRpc>>

/** Shared Bun-side Electrobun RPC handlers for the desktop app. */
export const appRpc: AppRpc = BrowserView.defineRPC<AppDesktopRpc>({
  handlers: {
    requests: {
      runtimeInfo: async () => ({ runtime: "electrobun" }),
      browseForProject: async () => ({ path: await browseForProject() }),
      getProjectGitStatus: async ({ path }) => await getProjectGitStatus(path),
      loadAppStateSnapshot: async () => ({ snapshot: await loadAppStateSnapshot() }),
      writeAppStateSnapshot: async ({ snapshot }) => {
        await writeAppStateSnapshot(snapshot)
        return {}
      },
      loadShortcutKeymap: async () => ({ keymap: await loadShortcutKeymap() }),
      writeShortcutKeymap: async ({ keymap }) => {
        await writeShortcutKeymap(keymap)
        return {}
      },
      listWorktreeOpeners: async (input) => await listWorktreeOpeners(input),
      openWorktree: async (input) => await openWorktree(input),
      daemonSend: async (input) => await daemonSend(input),
      daemonSubscribe: async (input) => await daemonSubscribe(input),
      daemonUnsubscribe: async (input) => await daemonUnsubscribe(input),
      daemonResetSubscriptions: async (input) => await daemonResetSubscriptions(input),
      maximizeWindow: async () => {
        getMainWindow()?.maximize()
        return {}
      },
      openExternal: async ({ url }) => ({ opened: Utils.openExternal(url) }),
      writeAppLog: async (input) => {
        await writeAppLog(input)
        return {}
      },
    },
    messages: {},
  },
})

/** Sends one typed global event from the Bun host into the active webview. */
export function dispatchGlobalEvent(event: GlobalEventEnvelope) {
  appRpc.send.dispatchGlobalEvent(event)
}
