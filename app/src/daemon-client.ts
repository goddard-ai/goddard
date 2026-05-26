import type { GoddardClient } from "@goddard-ai/sdk"

import type { DaemonRequestName, DaemonRequestPayload } from "~/shared/desktop-rpc.ts"
import { daemonSend, daemonSubscribe } from "./desktop-host.ts"

/** Browser-safe daemon client adapter backed by the Electrobun Bun host bridge. */
export const desktopDaemonClient = createDesktopDaemonClient() as GoddardClient

function createDesktopDaemonClient() {
  return new Proxy(
    {},
    {
      get(_target, property) {
        return createDesktopRouteNode([String(property)])
      },
    },
  )
}

function createDesktopRouteNode(path: readonly string[]): unknown {
  const route = async (
    payload: DaemonRequestPayload<DaemonRequestName> = undefined,
    options?: { signal?: AbortSignal },
  ) => {
    const name = path.join(".") as DaemonRequestName
    if (!options?.signal) {
      return await daemonSend(name, payload)
    }

    return createDesktopStream(name, payload, options.signal)
  }

  return new Proxy(route, {
    get(_target, property) {
      if (property === "then") {
        return undefined
      }

      return createDesktopRouteNode([...path, String(property)])
    },
  })
}

async function* createDesktopStream(
  name: DaemonRequestName,
  filter: DaemonRequestPayload<DaemonRequestName>,
  signal: AbortSignal,
) {
  const queue: unknown[] = []
  let wake: (() => void) | undefined
  const unsubscribe = await daemonSubscribe({ name, filter }, (payload) => {
    queue.push(payload)
    wake?.()
  })
  const abort = () => {
    wake?.()
  }

  signal.addEventListener("abort", abort)
  try {
    while (!signal.aborted) {
      const payload = queue.shift()
      if (payload !== undefined) {
        yield payload
        continue
      }
      await new Promise<void>((resolve) => {
        wake = resolve
      })
      wake = undefined
    }
  } finally {
    signal.removeEventListener("abort", abort)
    await Promise.resolve(unsubscribe()).catch(() => {})
  }
}
