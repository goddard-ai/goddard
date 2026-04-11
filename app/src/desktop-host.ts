import type { InferStreamPayload, StreamTarget } from "@goddard-ai/ipc";
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc";
import type { GoddardSdk } from "@goddard-ai/sdk";
import { Electroview } from "electrobun/view";

import type {
  AppDesktopRpc,
  DaemonResetSubscriptionsInput,
  DaemonRequestName,
  DaemonRequestPayload,
  DaemonRequestResponse,
  DaemonSendInput,
  DaemonStreamTargetInput,
  DaemonSubscribeInput,
  DaemonUnsubscribeInput,
  ReadShortcutKeymapResponse,
  RuntimeInfo,
} from "~/shared/desktop-rpc.ts";
import {
  type DaemonStreamName,
  dispatchGlobalEvent,
  globalEventHub,
} from "~/shared/global-event-hub.ts";
import { createDaemonSubscriptionCoordinator } from "~/shared/daemon-subscriptions.ts";
import type { UserShortcutKeymapFile } from "~/shared/shortcut-keymap.ts";
import { goddardSdk } from "./sdk.ts";

type DaemonSchema = typeof daemonIpcSchema;

const rpc = Electroview.defineRPC<AppDesktopRpc>({
  // Native dialogs and host-side daemon work can legitimately outlive Electrobun's
  // default 1s request timeout, so the app bridge must wait for the host response.
  maxRequestTime: Infinity,
  handlers: {
    requests: {},
    messages: {
      dispatchGlobalEvent,
    },
  },
});

let electroview: Electroview<typeof rpc> | undefined;
let daemonSubscriptionCoordinator:
  | ReturnType<typeof createDaemonSubscriptionCoordinator>
  | undefined;
let didRegisterDaemonResetOnUnload = false;

/** Browser-facing desktop bridge methods used by the app and manual smoke checks. */
export interface DesktopHostBridge {
  /** Returns the active desktop runtime reported by the Bun host. */
  getRuntimeInfo(): Promise<RuntimeInfo>;

  /** Opens one native directory picker and returns the chosen project root when present. */
  browseForProject(): Promise<string | null>;

  /** Reads the persisted user shortcut keymap through the Bun host bridge. */
  readShortcutKeymap(): Promise<ReadShortcutKeymapResponse>;

  /** Writes the persisted user shortcut keymap through the Bun host bridge. */
  writeShortcutKeymap(keymap: UserShortcutKeymapFile): Promise<UserShortcutKeymapFile>;

  /** Maximizes the active desktop window through the Bun host bridge. */
  maximizeWindow(): Promise<void>;

  /** Forwards one daemon IPC request through the Bun host's default daemon client. */
  daemonSend<Name extends DaemonRequestName>(
    name: Name,
    payload: DaemonRequestPayload<Name>,
  ): Promise<DaemonRequestResponse<Name>>;

  /** Opens one daemon IPC stream subscription through the Bun host bridge. */
  daemonSubscribe<Name extends DaemonStreamName>(
    target: StreamTarget<DaemonSchema, Name>,
    onMessage: (payload: InferStreamPayload<DaemonSchema, Name>) => void,
  ): Promise<() => void>;

  /** Shared SDK instance backed by the Bun-owned daemon client bridge. */
  sdk: GoddardSdk;
}

declare global {
  interface Window {
    __goddardDesktop: DesktopHostBridge;
  }
}

function getDaemonSubscriptionCoordinator() {
  if (daemonSubscriptionCoordinator) {
    return daemonSubscriptionCoordinator;
  }

  daemonSubscriptionCoordinator = createDaemonSubscriptionCoordinator({
    webviewId: window.__electrobunWebviewId,
    onUnsubscribeError(error) {
      console.error("Failed to unsubscribe from daemon stream.", error);
    },
    resetSubscriptions: async (input: DaemonResetSubscriptionsInput) =>
      await rpc.request.daemonResetSubscriptions(input),
    subscribe: async (input: DaemonSubscribeInput) => await rpc.request.daemonSubscribe(input),
    unsubscribe: async (input: DaemonUnsubscribeInput) =>
      await rpc.request.daemonUnsubscribe(input),
  });

  globalEventHub.on("daemonStream", (detail) => {
    daemonSubscriptionCoordinator?.dispatchEvent(detail);
  });

  return daemonSubscriptionCoordinator;
}

/** Creates the Electrobun view bridge once for the active browser context. */
export function initializeDesktopHost(): void {
  electroview ??= new Electroview({ rpc });
  if (!didRegisterDaemonResetOnUnload) {
    didRegisterDaemonResetOnUnload = true;
    window.addEventListener(
      "beforeunload",
      () => {
        void rpc.request
          .daemonResetSubscriptions({ webviewId: window.__electrobunWebviewId })
          .catch((error) => {
            console.error("Failed to reset daemon stream subscriptions during unload.", error);
          });
      },
      { once: true },
    );
  }

  void getDaemonSubscriptionCoordinator()
    .reset()
    .catch((error) => {
      console.error("Failed to reset daemon stream subscriptions.", error);
    });
}

/** Returns one runtime handshake from the Bun host. */
export async function getRuntimeInfo(): Promise<RuntimeInfo> {
  return await rpc.request.runtimeInfo({});
}

/** Opens one native directory picker for project selection. */
export async function browseForProject(): Promise<string | null> {
  const response = await rpc.request.browseForProject({});
  return response.path;
}

/** Reads the persisted user shortcut keymap through the Bun host. */
export async function readShortcutKeymap() {
  return await rpc.request.readShortcutKeymap({});
}

/** Writes the persisted user shortcut keymap through the Bun host. */
export async function writeShortcutKeymap(keymap: UserShortcutKeymapFile) {
  const response = await rpc.request.writeShortcutKeymap({ keymap });
  return response.keymap;
}

/** Maximizes the active desktop window through the Bun host. */
export async function maximizeWindow(): Promise<void> {
  await rpc.request.maximizeWindow({});
}

/** Forwards one daemon IPC request through the Bun host. */
export async function daemonSend<Name extends DaemonRequestName>(
  name: Name,
  payload: DaemonRequestPayload<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const input: DaemonSendInput<Name> = { name, payload };
  return (await rpc.request.daemonSend(input)) as DaemonRequestResponse<Name>;
}

function normalizeDaemonStreamTarget<Name extends DaemonStreamName>(
  target: StreamTarget<DaemonSchema, Name>,
): DaemonStreamTargetInput<Name> {
  if (typeof target === "string") {
    return {
      name: target,
      filter: undefined,
    } as DaemonStreamTargetInput<Name>;
  }

  return {
    name: target.name,
    filter: target.filter,
  } as DaemonStreamTargetInput<Name>;
}

/** Opens one daemon IPC stream subscription through the Bun host bridge. */
export async function daemonSubscribe<Name extends DaemonStreamName>(
  target: StreamTarget<DaemonSchema, Name>,
  onMessage: (payload: InferStreamPayload<DaemonSchema, Name>) => void,
): Promise<() => void> {
  return await getDaemonSubscriptionCoordinator().subscribe(
    normalizeDaemonStreamTarget(target),
    onMessage,
  );
}

/** Shared browser-side desktop host adapter for the current webview. */
export const desktopHost: DesktopHostBridge = {
  getRuntimeInfo,
  browseForProject,
  readShortcutKeymap,
  writeShortcutKeymap,
  maximizeWindow,
  daemonSend,
  daemonSubscribe,
  // Resolve lazily so the desktop bridge and SDK transport can share a module cycle safely.
  get sdk() {
    return goddardSdk;
  },
};
