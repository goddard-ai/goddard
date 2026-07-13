import { writeRendererDebug, writeRendererError } from "./renderer-log-capture.ts"

const INITIAL_RETRY_DELAY_MS = 250
const MAX_RETRY_DELAY_MS = 30_000

type DaemonEventStreamInput<TEvent> = {
  failureLogMessage: string
  logProperties?: Record<string, unknown>
  streamName: string
  open: (signal: AbortSignal) => Promise<AsyncIterable<TEvent>>
  reconcile: (signal: AbortSignal) => Promise<void> | void
  onEvent: (event: TEvent) => Promise<void> | void
}

/** Runs a daemon event stream until stopped, reconciling durable state after interruptions. */
export function startDaemonEventStream<TEvent>(input: DaemonEventStreamInput<TEvent>) {
  const controller = new AbortController()

  void runDaemonEventStream(input, controller.signal)

  return () => {
    controller.abort()
  }
}

async function runDaemonEventStream<TEvent>(
  input: DaemonEventStreamInput<TEvent>,
  signal: AbortSignal,
) {
  let hasOpened = false
  let retryAttempt = 0

  while (!signal.aborted) {
    const attemptController = new AbortController()
    const abortAttempt = () => attemptController.abort()
    signal.addEventListener("abort", abortAttempt, { once: true })

    try {
      const events = await input.open(attemptController.signal)
      if (attemptController.signal.aborted) {
        return
      }

      if (hasOpened) {
        await input.reconcile(attemptController.signal)
        if (attemptController.signal.aborted) {
          return
        }

        writeRendererDebug("app.daemon_event_stream", "app.daemon_event_stream.recovered", {
          ...input.logProperties,
          retryAttempt,
          streamName: input.streamName,
        })
      }
      hasOpened = true

      for await (const event of events) {
        if (attemptController.signal.aborted) {
          return
        }

        retryAttempt = 0
        await input.onEvent(event)
      }

      if (!attemptController.signal.aborted) {
        throw new Error("Daemon event stream ended unexpectedly.")
      }
    } catch (error) {
      if (attemptController.signal.aborted) {
        return
      }

      retryAttempt += 1
      const retryDelayMs = Math.min(
        INITIAL_RETRY_DELAY_MS * 2 ** (retryAttempt - 1),
        MAX_RETRY_DELAY_MS,
      )
      const properties = {
        ...input.logProperties,
        retryAttempt,
        retryDelayMs,
        streamName: input.streamName,
      }

      writeRendererError(input.failureLogMessage, error, properties)

      attemptController.abort()
      await waitForRetry(retryDelayMs, signal)
    } finally {
      signal.removeEventListener("abort", abortAttempt)
      attemptController.abort()
    }
  }
}

function waitForRetry(delayMs: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    if (signal.aborted) {
      resolve()
      return
    }

    const timeout = setTimeout(finish, delayMs)

    function finish() {
      clearTimeout(timeout)
      signal.removeEventListener("abort", finish)
      resolve()
    }

    signal.addEventListener("abort", finish, { once: true })
  })
}
