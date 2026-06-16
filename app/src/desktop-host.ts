import type { GoddardSdk } from "@goddard-ai/sdk"
import { Electroview } from "electrobun/view"
import { listen } from "preact-sigma"

import type { AppStateSnapshot } from "~/shared/app-state.ts"
import { createDaemonSubscriptionCoordinator } from "~/shared/daemon-subscriptions.ts"
import type {
  AppDesktopRpc,
  AppLogInput,
  DaemonRequestName,
  DaemonRequestPayload,
  DaemonRequestResponse,
  DaemonSendInput,
  DaemonStreamTargetInput,
  ProjectGitStatus,
  RuntimeInfo,
} from "~/shared/desktop-rpc.ts"
import { globalEventHub, type DaemonStreamName } from "~/shared/global-event-hub.ts"
import type { ShortcutKeymapFile } from "~/shared/shortcut-keymap.ts"
import { goddardSdk } from "./sdk.ts"

const rpc = Electroview.defineRPC<AppDesktopRpc>({
  // Native dialogs and host-side daemon work can legitimately outlive Electrobun's
  // default 1s request timeout, so the app bridge must wait for the host response.
  maxRequestTime: Infinity,
  handlers: {
    requests: {},
    messages: {
      dispatchGlobalEvent(event) {
        globalEventHub.emit(event.name, event.detail)
      },
    },
  },
})

const consoleMethods: AppLogInput["level"][] = ["debug", "error", "info", "log", "warn"]
let electroview: Electroview<typeof rpc> | undefined
let daemonSubscriptionCoordinator:
  | ReturnType<typeof createDaemonSubscriptionCoordinator>
  | undefined
let didRegisterDaemonResetOnUnload = false

/** Browser-facing desktop bridge methods used by the app and manual smoke checks. */
export interface DesktopHostBridge {
  /** Returns the active desktop runtime reported by the Bun host. */
  getRuntimeInfo(): Promise<RuntimeInfo>

  /** Opens one native directory picker and returns the chosen project root when present. */
  browseForProject(): Promise<string | null>

  /** Reads git status summaries for one project root through the Bun host bridge. */
  getProjectGitStatus(path: string): Promise<ProjectGitStatus>

  /** Reads the app-state snapshot through the Bun host bridge. */
  loadAppStateSnapshot<T extends AppStateSnapshot>(): Promise<T | null>

  /** Writes the app-state snapshot through the Bun host bridge. */
  writeAppStateSnapshot(snapshot: AppStateSnapshot): Promise<void>

  /** Reads the app-only shortcut keymap file through the Bun host bridge. */
  loadShortcutKeymap(): Promise<ShortcutKeymapFile | null>

  /** Writes the app-only shortcut keymap file through the Bun host bridge. */
  writeShortcutKeymap(keymap: ShortcutKeymapFile): Promise<void>

  /** Maximizes the active desktop window through the Bun host bridge. */
  maximizeWindow(): Promise<void>

  /** Opens one URL through the operating system default browser or URL handler. */
  openExternal(url: string): Promise<boolean>

  /** Forwards one daemon IPC request through the Bun host's default daemon client. */
  daemonSend<Name extends DaemonRequestName>(
    name: Name,
    payload: DaemonRequestPayload<Name>,
  ): Promise<DaemonRequestResponse<Name>>

  /** Opens one daemon IPC stream subscription through the Bun host bridge. */
  daemonSubscribe<Name extends DaemonStreamName>(
    target: DaemonStreamTargetInput<Name>,
    onMessage: (payload: any) => void,
  ): Promise<() => void>

  /** Shared SDK instance backed by the Bun-owned daemon client bridge. */
  sdk: GoddardSdk
}

declare global {
  interface Window {
    __goddardDesktop: DesktopHostBridge
    __goddardDidInstallLogCapture?: boolean
  }
}

function getDaemonSubscriptionCoordinator() {
  if (daemonSubscriptionCoordinator) {
    return daemonSubscriptionCoordinator
  }

  daemonSubscriptionCoordinator = createDaemonSubscriptionCoordinator({
    webviewId: window.__electrobunWebviewId,
    onUnsubscribeError(error) {
      console.error("Failed to unsubscribe from daemon stream.", error)
    },
    resetSubscriptions: (input) => rpc.request.daemonResetSubscriptions(input),
    subscribe: (input) => rpc.request.daemonSubscribe(input),
    unsubscribe: (input) => rpc.request.daemonUnsubscribe(input),
  })

  listen(globalEventHub, "daemonStream", (detail) => {
    daemonSubscriptionCoordinator?.dispatchEvent(detail)
  })

  return daemonSubscriptionCoordinator
}

/** Creates the Electrobun view bridge once for the active browser context. */
export function initializeDesktopHost(): void {
  electroview ??= new Electroview({ rpc })
  installRendererLogCapture()

  if (!didRegisterDaemonResetOnUnload) {
    didRegisterDaemonResetOnUnload = true
    window.addEventListener(
      "beforeunload",
      () => {
        void rpc.request
          .daemonResetSubscriptions({ webviewId: window.__electrobunWebviewId })
          .catch((error) => {
            console.error("Failed to reset daemon stream subscriptions during unload.", error)
          })
      },
      { once: true },
    )
  }

  void getDaemonSubscriptionCoordinator()
    .reset()
    .catch((error) => {
      console.error("Failed to reset daemon stream subscriptions.", error)
    })
}

function installRendererLogCapture() {
  if (window.__goddardDidInstallLogCapture) {
    return
  }

  window.__goddardDidInstallLogCapture = true

  for (const method of consoleMethods) {
    const original = console[method].bind(console)
    console[method] = (...args: unknown[]) => {
      void writeRendererLog(method, args.map(formatConsoleValue).join(" "))
      original(...args)
    }
  }

  window.addEventListener("error", (event) => {
    void writeRendererLog("error", formatErrorEvent(event))
  })
  window.addEventListener("unhandledrejection", (event) => {
    void writeRendererLog("error", formatConsoleValue(event.reason))
  })
}

async function writeRendererLog(level: AppLogInput["level"], message: string) {
  await rpc.request
    .writeAppLog({
      source: "renderer",
      level,
      message,
      webviewId: window.__electrobunWebviewId,
    })
    .catch(() => {})
}

function formatErrorEvent(event: ErrorEvent) {
  return event.error ? formatConsoleValue(event.error) : event.message
}

function formatConsoleValue(value: unknown) {
  if (typeof value === "string") {
    return value
  }

  if (value instanceof Error) {
    return value.stack ?? value.message
  }

  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}

/** Returns one runtime handshake from the Bun host. */
export async function getRuntimeInfo(): Promise<RuntimeInfo> {
  return await rpc.request.runtimeInfo({})
}

/** Opens one native directory picker for project selection. */
export async function browseForProject(): Promise<string | null> {
  const response = await rpc.request.browseForProject({})
  return response.path
}

/** Reads git status summaries for one project root through the Bun host. */
export async function getProjectGitStatus(path: string): Promise<ProjectGitStatus> {
  return await rpc.request.getProjectGitStatus({ path })
}

/** Reads the app-state snapshot through the Bun host bridge. */
export async function loadAppStateSnapshot<T extends AppStateSnapshot>() {
  const response = await rpc.request.loadAppStateSnapshot({})
  return response.snapshot as T | null
}

/** Writes the app-state snapshot through the Bun host bridge. */
export async function writeAppStateSnapshot(snapshot: AppStateSnapshot) {
  await rpc.request.writeAppStateSnapshot({ snapshot })
}

/** Reads the app-only shortcut keymap file through the Bun host bridge. */
export async function loadShortcutKeymap() {
  const response = await rpc.request.loadShortcutKeymap({})
  return response.keymap
}

/** Writes the app-only shortcut keymap file through the Bun host bridge. */
export async function writeShortcutKeymap(keymap: ShortcutKeymapFile) {
  await rpc.request.writeShortcutKeymap({ keymap })
}

/** Maximizes the active desktop window through the Bun host. */
export async function maximizeWindow(): Promise<void> {
  await rpc.request.maximizeWindow({})
}

/** Opens one URL through the operating system default browser or URL handler. */
export async function openExternal(url: string): Promise<boolean> {
  const response = await rpc.request.openExternal({ url })
  return response.opened
}

/** Forwards one daemon IPC request through the Bun host. */
export async function daemonSend<Name extends DaemonRequestName>(
  name: Name,
  payload: DaemonRequestPayload<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const input: DaemonSendInput<Name> = { name, payload }
  return (await rpc.request.daemonSend(input)) as DaemonRequestResponse<Name>
}

function normalizeDaemonStreamTarget<Name extends DaemonStreamName>(
  target: Name | DaemonStreamTargetInput<Name>,
): DaemonStreamTargetInput<Name> {
  if (typeof target === "string") {
    return {
      name: target,
      filter: undefined,
    } as DaemonStreamTargetInput<Name>
  }

  return {
    name: target.name,
    filter: target.filter,
  } as DaemonStreamTargetInput<Name>
}

/** Opens one daemon IPC stream subscription through the Bun host bridge. */
export async function daemonSubscribe<Name extends DaemonStreamName>(
  target: Name | DaemonStreamTargetInput<Name>,
  onMessage: (payload: any) => void,
): Promise<() => void> {
  return await getDaemonSubscriptionCoordinator().subscribe(
    normalizeDaemonStreamTarget(target),
    onMessage,
  )
}

/** Shared browser-side desktop host adapter for the current webview. */
export const desktopHost: DesktopHostBridge = {
  getRuntimeInfo,
  browseForProject,
  getProjectGitStatus,
  loadAppStateSnapshot,
  writeAppStateSnapshot,
  loadShortcutKeymap,
  writeShortcutKeymap,
  maximizeWindow,
  openExternal,
  daemonSend,
  daemonSubscribe,
  // Resolve lazily so the desktop bridge and SDK transport can share a module cycle safely.
  get sdk() {
    return goddardSdk
  },
}
