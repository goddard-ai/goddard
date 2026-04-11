import type {
  InferRequestPayload,
  InferResponseType,
  InferStreamFilter,
  ValidRequestName,
} from "@goddard-ai/ipc";
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc";
import type { RPCSchema } from "electrobun/bun";

import type { UserShortcutKeymapFile } from "./shortcut-keymap.ts";
import type { DaemonStreamName, GlobalEventEnvelope } from "./global-event-hub.ts";

/** Daemon IPC schema type reused for webview-to-Bun request forwarding. */
type DaemonSchema = typeof daemonIpcSchema;

/** Valid daemon IPC request names forwarded through the desktop host. */
export type DaemonRequestName = ValidRequestName<DaemonSchema>;

/** Payload type for one forwarded daemon IPC request. */
export type DaemonRequestPayload<Name extends DaemonRequestName = DaemonRequestName> =
  InferRequestPayload<DaemonSchema, Name>;

/** Response type for one forwarded daemon IPC request. */
export type DaemonRequestResponse<Name extends DaemonRequestName = DaemonRequestName> =
  InferResponseType<DaemonSchema, Name>;

/** One normalized daemon IPC stream target forwarded through the Electrobun bridge. */
export type DaemonStreamTargetInput<Name extends DaemonStreamName = DaemonStreamName> = {
  name: Name;
  filter: InferStreamFilter<DaemonSchema, Name> | undefined;
};

/** Bun-host RPC payload for forwarding one daemon IPC request. */
export type DaemonSendInput<Name extends DaemonRequestName = DaemonRequestName> = {
  name: Name;
  payload: DaemonRequestPayload<Name>;
};

/** Bun-host RPC payload for opening one daemon IPC stream subscription. */
export type DaemonSubscribeInput<Name extends DaemonStreamName = DaemonStreamName> = {
  webviewId: number;
  subscriptionId: string;
  target: DaemonStreamTargetInput<Name>;
};

/** Bun-host RPC payload for closing one daemon IPC stream subscription. */
export type DaemonUnsubscribeInput = {
  subscriptionId: string;
};

/** Bun-host RPC payload for clearing every daemon stream subscription owned by one webview. */
export type DaemonResetSubscriptionsInput = {
  webviewId: number;
};

/** Minimal runtime information exposed by the Electrobun Bun host. */
export type RuntimeInfo = {
  runtime: "electrobun";
};

/** Result of loading the persisted user shortcut keymap from the Bun host. */
export type ReadShortcutKeymapResponse = {
  keymap: UserShortcutKeymapFile | null;
  error: string | null;
};

/** Shared Electrobun RPC contract between the Bun host and the browser view. */
export type AppDesktopRpc = {
  bun: RPCSchema<{
    requests: {
      runtimeInfo: {
        params: {};
        response: RuntimeInfo;
      };
      browseForProject: {
        params: {};
        response: { path: string | null };
      };
      readShortcutKeymap: {
        params: {};
        response: ReadShortcutKeymapResponse;
      };
      writeShortcutKeymap: {
        params: { keymap: UserShortcutKeymapFile };
        response: { keymap: UserShortcutKeymapFile };
      };
      daemonSend: {
        params: DaemonSendInput;
        response: unknown;
      };
      daemonSubscribe: {
        params: DaemonSubscribeInput;
        response: { subscriptionId: string };
      };
      daemonUnsubscribe: {
        params: DaemonUnsubscribeInput;
        response: { removed: boolean };
      };
      daemonResetSubscriptions: {
        params: DaemonResetSubscriptionsInput;
        response: { removedCount: number };
      };
      maximizeWindow: {
        params: {};
        response: {};
      };
    };
    messages: {};
  }>;
  webview: RPCSchema<{
    requests: {};
    messages: {
      dispatchGlobalEvent: GlobalEventEnvelope;
    };
  }>;
};
