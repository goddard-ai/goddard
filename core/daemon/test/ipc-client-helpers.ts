import type { DaemonIpcClient } from "@goddard-ai/daemon-client"

/** Test-only dotted-name dispatcher for legacy daemon test cases. */
export function send(client: DaemonIpcClient, name: string, payload?: unknown) {
  return selectRouteFunction(client, name)(payload)
}

/** Test-only stream bridge for legacy daemon test cases. */
export async function subscribe(
  client: DaemonIpcClient,
  target: string | { readonly name: string; readonly filter?: unknown },
  onMessage: (payload: any) => void,
) {
  const abortController = new AbortController()
  const name = typeof target === "string" ? target : target.name
  const filter = typeof target === "string" ? undefined : target.filter
  const stream = (await selectRouteFunction(client, name)(filter, {
    signal: abortController.signal,
  })) as AsyncIterable<unknown>
  const iterator = stream[Symbol.asyncIterator]()
  const done = (async () => {
    while (true) {
      const result = await iterator.next()
      if (result.done) {
        return
      }
      const payload = result.value
      onMessage(payload)
    }
  })()

  return () => {
    abortController.abort()
    return Promise.all([iterator.return?.(), done.catch(() => {})]).then(() => {})
  }
}

function selectRouteFunction(client: DaemonIpcClient, name: string) {
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
