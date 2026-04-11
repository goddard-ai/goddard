import { BrowserView } from "electrobun/bun";
import { createDaemonIpcClient, type DaemonIpcClient } from "@goddard-ai/daemon-client/node";

import type {
  DaemonResetSubscriptionsInput,
  DaemonRequestName,
  DaemonRequestResponse,
  DaemonSendInput,
  DaemonSubscribeInput,
  DaemonUnsubscribeInput,
} from "~/shared/desktop-rpc.ts";
import type { GlobalEventEnvelope } from "~/shared/global-event-hub.ts";
import { ensureDaemonRuntime } from "./daemon-runtime.ts";

let daemonClient: DaemonIpcClient | undefined;
const daemonStreamSubscriptions = new Map<
  string,
  {
    unsubscribe: () => void;
    webviewId: number;
  }
>();
const daemonSubscriptionIdsByWebview = new Map<number, Set<string>>();

/** Reuses one daemon IPC client for the Bun host process. */
async function getDaemonClient() {
  if (daemonClient) {
    return daemonClient;
  }

  const client = createDaemonIpcClient(await ensureDaemonRuntime());
  daemonClient = client;
  return client;
}

function publishGlobalEvent(webviewId: number, event: GlobalEventEnvelope) {
  const browserView = BrowserView.getById(webviewId) as
    | {
        rpc?: {
          send?: {
            dispatchGlobalEvent?: (payload: GlobalEventEnvelope) => void;
          };
        };
      }
    | undefined;

  browserView?.rpc?.send?.dispatchGlobalEvent?.(event);
}

function addDaemonSubscriptionOwner(webviewId: number, subscriptionId: string) {
  const existingIds = daemonSubscriptionIdsByWebview.get(webviewId);

  if (existingIds) {
    existingIds.add(subscriptionId);
    return;
  }

  daemonSubscriptionIdsByWebview.set(webviewId, new Set([subscriptionId]));
}

function removeDaemonSubscriptionOwner(webviewId: number, subscriptionId: string) {
  const existingIds = daemonSubscriptionIdsByWebview.get(webviewId);

  if (!existingIds) {
    return;
  }

  existingIds.delete(subscriptionId);

  if (existingIds.size === 0) {
    daemonSubscriptionIdsByWebview.delete(webviewId);
  }
}

async function removeDaemonSubscription(subscriptionId: string) {
  const subscription = daemonStreamSubscriptions.get(subscriptionId);

  if (!subscription) {
    return false;
  }

  daemonStreamSubscriptions.delete(subscriptionId);
  removeDaemonSubscriptionOwner(subscription.webviewId, subscriptionId);
  await Promise.resolve(subscription.unsubscribe()).catch((error) => {
    console.error(`Failed to close daemon stream subscription ${subscriptionId}.`, error);
  });
  return true;
}

/** Forwards one daemon IPC request through the Bun host's default daemon client. */
export async function daemonSend<Name extends DaemonRequestName>(
  input: DaemonSendInput<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const client = await getDaemonClient();
  const send = client.send as (
    name: Name,
    payload?: DaemonSendInput<Name>["payload"],
  ) => Promise<unknown>;
  return (await send(input.name, input.payload)) as DaemonRequestResponse<Name>;
}

/** Opens one daemon IPC stream subscription on behalf of one Electrobun webview. */
export async function daemonSubscribe(input: DaemonSubscribeInput) {
  if (!BrowserView.getById(input.webviewId)) {
    throw new Error(`Missing BrowserView for webview ${input.webviewId}.`);
  }

  const client = await getDaemonClient();
  await removeDaemonSubscription(input.subscriptionId);

  const unsubscribe = await client.subscribe(input.target as never, (payload) => {
    if (!daemonStreamSubscriptions.has(input.subscriptionId)) {
      return;
    }

    publishGlobalEvent(input.webviewId, {
      name: "daemonStream",
      detail: {
        subscriptionId: input.subscriptionId,
        name: input.target.name,
        payload,
      },
    });
  });

  daemonStreamSubscriptions.set(input.subscriptionId, {
    unsubscribe,
    webviewId: input.webviewId,
  });
  addDaemonSubscriptionOwner(input.webviewId, input.subscriptionId);

  return {
    subscriptionId: input.subscriptionId,
  };
}

/** Closes one daemon IPC stream subscription opened by the Bun host. */
export async function daemonUnsubscribe(input: DaemonUnsubscribeInput) {
  return {
    removed: await removeDaemonSubscription(input.subscriptionId),
  };
}

/** Clears every daemon IPC stream subscription currently owned by one Electrobun webview. */
export async function daemonResetSubscriptions(input: DaemonResetSubscriptionsInput) {
  const subscriptionIds = [...(daemonSubscriptionIdsByWebview.get(input.webviewId) ?? [])];

  await Promise.all(
    subscriptionIds.map((subscriptionId) => removeDaemonSubscription(subscriptionId)),
  );

  return {
    removedCount: subscriptionIds.length,
  };
}
