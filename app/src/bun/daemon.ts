import { createDaemonIpcClient, type DaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { BrowserView, defineElectrobunRPC } from "electrobun/bun"

import type {
  AppDesktopRpc,
  DaemonRequestName,
  DaemonRequestResponse,
  DaemonResetSubscriptionsInput,
  DaemonSendInput,
  DaemonSubscribeInput,
  DaemonUnsubscribeInput,
} from "~/shared/desktop-rpc.ts"
import type { GlobalEventEnvelope } from "~/shared/global-event-hub.ts"
import { ensureDaemonRuntime } from "./daemon-runtime.ts"
import { createClientIpcLogHook } from "./ipc-client-logging.ts"

let daemonClient: DaemonIpcClient | undefined
const daemonStreamSubscriptions = new Map<
  string,
  {
    unsubscribe: () => void
    webviewId: number
  }
>()
const daemonSubscriptionIdsByWebview = new Map<number, Set<string>>()

/** Reuses one daemon IPC client for the Bun host process. */
async function getDaemonClient() {
  if (daemonClient) {
    return daemonClient
  }

  const client = createDaemonIpcClient({
    ...(await ensureDaemonRuntime()),
    ipcHook: createClientIpcLogHook(),
  })
  daemonClient = client
  return client
}

type BrowserViewRpc = ReturnType<typeof defineElectrobunRPC<AppDesktopRpc, "bun">>

function publishGlobalEvent(webviewId: number, event: GlobalEventEnvelope) {
  const browserView = BrowserView.getById(webviewId) as BrowserView<BrowserViewRpc> | undefined

  browserView?.rpc?.send.dispatchGlobalEvent(event)
}

function addDaemonSubscriptionOwner(webviewId: number, subscriptionId: string) {
  const existingIds = daemonSubscriptionIdsByWebview.get(webviewId)

  if (existingIds) {
    existingIds.add(subscriptionId)
    return
  }

  daemonSubscriptionIdsByWebview.set(webviewId, new Set([subscriptionId]))
}

function removeDaemonSubscriptionOwner(webviewId: number, subscriptionId: string) {
  const existingIds = daemonSubscriptionIdsByWebview.get(webviewId)

  if (!existingIds) {
    return
  }

  existingIds.delete(subscriptionId)

  if (existingIds.size === 0) {
    daemonSubscriptionIdsByWebview.delete(webviewId)
  }
}

async function removeDaemonSubscription(subscriptionId: string) {
  const subscription = daemonStreamSubscriptions.get(subscriptionId)

  if (!subscription) {
    return false
  }

  subscription.unsubscribe()
  daemonStreamSubscriptions.delete(subscriptionId)
  removeDaemonSubscriptionOwner(subscription.webviewId, subscriptionId)

  return true
}

/** Forwards one daemon IPC request through the Bun host's default daemon client. */
export async function daemonSend<Name extends DaemonRequestName>(
  input: DaemonSendInput<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const client = await getDaemonClient()
  return (await selectRouteFunction(
    client,
    input.name,
  )(input.payload)) as DaemonRequestResponse<Name>
}

/** Opens one daemon IPC stream subscription on behalf of one Electrobun webview. */
export async function daemonSubscribe(input: DaemonSubscribeInput) {
  if (!BrowserView.getById(input.webviewId)) {
    throw new Error(`Missing BrowserView for webview ${input.webviewId}.`)
  }

  const client = await getDaemonClient()
  await removeDaemonSubscription(input.subscriptionId)
  const abortController = new AbortController()
  const stream = (await selectRouteFunction(client, input.target.name)(input.target.filter, {
    signal: abortController.signal,
  })) as AsyncIterable<unknown>
  const done = (async () => {
    for await (const payload of stream) {
      if (!daemonStreamSubscriptions.has(input.subscriptionId)) {
        return
      }

      publishGlobalEvent(input.webviewId, {
        name: "daemonStream",
        detail: {
          subscriptionId: input.subscriptionId,
          name: input.target.name,
          payload,
        },
      })
    }
  })()

  daemonStreamSubscriptions.set(input.subscriptionId, {
    unsubscribe: () => {
      abortController.abort()
      void done.catch(() => {})
    },
    webviewId: input.webviewId,
  })
  addDaemonSubscriptionOwner(input.webviewId, input.subscriptionId)

  return {
    subscriptionId: input.subscriptionId,
  }
}

/** Closes one daemon IPC stream subscription opened by the Bun host. */
export async function daemonUnsubscribe(input: DaemonUnsubscribeInput) {
  return {
    removed: await removeDaemonSubscription(input.subscriptionId),
  }
}

/** Clears every daemon IPC stream subscription currently owned by one Electrobun webview. */
export async function daemonResetSubscriptions(input: DaemonResetSubscriptionsInput) {
  const subscriptionIds = [...(daemonSubscriptionIdsByWebview.get(input.webviewId) ?? [])]

  await Promise.all(
    subscriptionIds.map((subscriptionId) => removeDaemonSubscription(subscriptionId)),
  )

  return {
    removedCount: subscriptionIds.length,
  }
}

function selectRouteFunction(client: Record<string, any>, name: string) {
  let node: unknown = client
  for (const segment of name.split(".")) {
    if (!node || typeof node !== "object" || !(segment in node)) {
      throw new Error(`Unknown daemon IPC route: ${name}`)
    }
    node = (node as Record<string, unknown>)[segment]
  }

  if (typeof node !== "function") {
    throw new Error(`Daemon IPC route is not callable: ${name}`)
  }

  return node
}
