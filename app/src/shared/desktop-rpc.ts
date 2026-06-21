import type { RPCSchema } from "electrobun/bun"

import type { AppStateSnapshot } from "./app-state.ts"
import type { DaemonStreamName, GlobalEventEnvelope } from "./global-event-hub.ts"
import type { ShortcutKeymapFile } from "./shortcut-keymap.ts"

/** Valid daemon IPC request names forwarded through the desktop host. */
export type DaemonRequestName = string

/** Payload type for one forwarded daemon IPC request. */
export type DaemonRequestPayload<_Name extends DaemonRequestName = DaemonRequestName> = any

/** Response type for one forwarded daemon IPC request. */
export type DaemonRequestResponse<_Name extends DaemonRequestName = DaemonRequestName> = any

/** One normalized daemon IPC stream target forwarded through the Electrobun bridge. */
export type DaemonStreamTargetInput<Name extends DaemonStreamName = DaemonStreamName> = {
  name: Name
  filter: any
}

/** Bun-host RPC payload for forwarding one daemon IPC request. */
export type DaemonSendInput<Name extends DaemonRequestName = DaemonRequestName> = {
  name: Name
  payload: DaemonRequestPayload<Name>
}

/** Bun-host RPC payload for opening one daemon IPC stream subscription. */
export type DaemonSubscribeInput<Name extends DaemonStreamName = DaemonStreamName> = {
  webviewId: number
  subscriptionId: string
  target: DaemonStreamTargetInput<Name>
}

/** Bun-host RPC payload for closing one daemon IPC stream subscription. */
export type DaemonUnsubscribeInput = {
  subscriptionId: string
}

/** Bun-host RPC payload for clearing every daemon stream subscription owned by one webview. */
export type DaemonResetSubscriptionsInput = {
  webviewId: number
}

/** Host-bootstrapped daemon access credentials for direct desktop webview IPC. */
export type DaemonWebviewAccess = {
  daemonUrl: string
  token: string
  origin: string
  expiresAt: string
}

/** Bun-host RPC payload for issuing daemon access to the current desktop webview origin. */
export type DaemonWebviewAccessInput = {
  origin: string
}

/** Minimal runtime information exposed by the Electrobun Bun host. */
export type RuntimeInfo = {
  runtime: "electrobun"
}

/** Browser or Bun-host app log record forwarded into the shared temp log file. */
export type AppLogInput = {
  source: "host" | "renderer"
  level: "debug" | "error" | "info" | "log" | "warn"
  message: string
  debugScope?: string
  properties?: Record<string, unknown>
  webviewId?: number
}

/** Compact git checkout summary shown on project-scoped dashboard surfaces. */
export type ProjectGitCheckoutSummary = {
  path: string
  branch: string | null
  head: string | null
  hasChanges: boolean
  changedCount: number
  untrackedCount: number
  ahead: number
  behind: number
  errorMessage: string | null
}

/** Git status payload for one user-added project root and its linked worktrees. */
export type ProjectGitStatus = {
  primary: ProjectGitCheckoutSummary
  worktrees: ProjectGitCheckoutSummary[]
}

/** Shared Electrobun RPC contract between the Bun host and the browser view. */
export type AppDesktopRpc = {
  bun: RPCSchema<{
    requests: {
      runtimeInfo: {
        params: {}
        response: RuntimeInfo
      }
      browseForProject: {
        params: {}
        response: { path: string | null }
      }
      getProjectGitStatus: {
        params: { path: string }
        response: ProjectGitStatus
      }
      loadAppStateSnapshot: {
        params: {}
        response: { snapshot: AppStateSnapshot | null }
      }
      writeAppStateSnapshot: {
        params: { snapshot: AppStateSnapshot }
        response: {}
      }
      loadShortcutKeymap: {
        params: {}
        response: { keymap: ShortcutKeymapFile | null }
      }
      writeShortcutKeymap: {
        params: { keymap: ShortcutKeymapFile }
        response: {}
      }
      daemonSend: {
        params: DaemonSendInput
        response: unknown
      }
      daemonSubscribe: {
        params: DaemonSubscribeInput
        response: { subscriptionId: string }
      }
      daemonUnsubscribe: {
        params: DaemonUnsubscribeInput
        response: { removed: boolean }
      }
      daemonResetSubscriptions: {
        params: DaemonResetSubscriptionsInput
        response: { removedCount: number }
      }
      daemonWebviewAccess: {
        params: DaemonWebviewAccessInput
        response: DaemonWebviewAccess
      }
      mainWindowReady: {
        params: {}
        response: {}
      }
      maximizeWindow: {
        params: {}
        response: {}
      }
      openExternal: {
        params: { url: string }
        response: { opened: boolean }
      }
      writeAppLog: {
        params: AppLogInput
        response: {}
      }
    }
    messages: {}
  }>
  webview: RPCSchema<{
    requests: {}
    messages: {
      dispatchGlobalEvent: GlobalEventEnvelope
    }
  }>
}
