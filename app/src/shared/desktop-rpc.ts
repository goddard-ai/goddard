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

/** Minimal runtime information exposed by the Electrobun Bun host. */
export type RuntimeInfo = {
  runtime: "electrobun"
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
      maximizeWindow: {
        params: {}
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
