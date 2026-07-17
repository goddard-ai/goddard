import { expect, test } from "vitest"

import { startDaemonEventStream } from "./daemon-event-stream.ts"

test("retries interrupted streams and reconciles before consuming reconnected events", async () => {
  const steps: string[] = []
  let openCount = 0

  const stop = startDaemonEventStream({
    failureLogMessage: "test.events.subscription_failed",
    streamName: "test.events",
    open: async (signal) => {
      openCount += 1

      if (openCount === 1) {
        return (async function* () {
          yield "first"
          throw new Error("connection lost")
        })()
      }

      return (async function* () {
        yield "second"
        await waitForAbort(signal)
      })()
    },
    reconcile: () => {
      steps.push("reconcile")
    },
    onEvent: (event) => {
      steps.push(`event:${event}`)
    },
  })

  try {
    await waitFor(() => steps.includes("event:second"))

    expect(openCount).toBe(2)
    expect(steps).toEqual(["event:first", "reconcile", "event:second"])
  } finally {
    stop()
  }
})

test("stopping during retry backoff prevents another stream attempt", async () => {
  let openCount = 0

  const stop = startDaemonEventStream({
    failureLogMessage: "test.events.subscription_failed",
    streamName: "test.events",
    open: async () => {
      openCount += 1
      throw new Error("daemon unavailable")
    },
    reconcile: () => {},
    onEvent: () => {},
  })

  await waitFor(() => openCount === 1)
  stop()
  await new Promise((resolve) => setTimeout(resolve, 300))

  expect(openCount).toBe(1)
})

function waitForAbort(signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    if (signal.aborted) {
      resolve()
      return
    }

    signal.addEventListener("abort", () => resolve(), { once: true })
  })
}

async function waitFor(check: () => boolean) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (check()) {
      return
    }

    await new Promise((resolve) => setTimeout(resolve, 10))
  }

  throw new Error("Timed out waiting for condition.")
}
