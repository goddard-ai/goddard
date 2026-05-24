import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { workforceIpcRoutes } from "./daemon-ipc.ts"
import type {
  CancelWorkforceRequest,
  CreateWorkforceRequest,
  DiscoverWorkforceCandidatesRequest,
  GetWorkforceRequest,
  InitializeWorkforceRequest,
  RespondWorkforceRequest,
  ShutdownWorkforceRequest,
  StartWorkforceRequest,
  SubscribeWorkforceEventsRequest,
  SuspendWorkforceRequest,
  TruncateWorkforceRequest,
  UpdateWorkforceRequest,
  WorkforceEventEnvelope,
} from "./schema.ts"

export const workforceSdkPlugin = defineSdkPlugin({
  name: "workforce",
  ipcRoutes: workforceIpcRoutes,
  wrap({ client }) {
    return {
      workforce: {
        /** Starts or reuses one daemon workforce runtime. */
        start: (input: StartWorkforceRequest) => client.workforce.start({ body: input }),

        /** Discovers package candidates for one repository workforce initialization flow. */
        discoverCandidates: (input: DiscoverWorkforceCandidatesRequest) =>
          client.workforce.discoverCandidates({ body: input }),

        /** Initializes one repository workforce config and ledger through the daemon. */
        initialize: (input: InitializeWorkforceRequest) =>
          client.workforce.initialize({ body: input }),

        /** Fetches one daemon workforce runtime and its resolved config. */
        get: (input: GetWorkforceRequest) => client.workforce.get({ body: input }),

        /** Lists daemon workforce runtime summaries. */
        list: () => client.workforce.list(),

        /** Subscribes to live daemon-published workforce ledger events for one repository root. */
        subscribe: async (
          input: SubscribeWorkforceEventsRequest,
          onEvent: (event: WorkforceEventEnvelope["event"]) => void,
        ): Promise<() => void> => {
          const controller = new AbortController()
          const events = await client.workforce.event({ query: input, signal: controller.signal })
          void (async () => {
            for await (const payload of events) {
              if (controller.signal.aborted) {
                break
              }
              onEvent(payload.event)
            }
          })()
          return () => controller.abort()
        },

        /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
        shutdown: (input: ShutdownWorkforceRequest) => client.workforce.shutdown({ body: input }),

        /** Enqueues one workforce request and includes the updated workforce projection. */
        request: (input: CreateWorkforceRequest) => client.workforce.request({ body: input }),

        /** Updates one workforce request and includes the updated workforce projection. */
        update: (input: UpdateWorkforceRequest) => client.workforce.update({ body: input }),

        /** Cancels one workforce request and includes the updated workforce projection. */
        cancel: (input: CancelWorkforceRequest) => client.workforce.cancel({ body: input }),

        /** Truncates one workforce queue and includes the updated workforce projection. */
        truncate: (input: TruncateWorkforceRequest) => client.workforce.truncate({ body: input }),

        /** Responds to one active workforce request and includes the updated workforce projection. */
        respond: (input: RespondWorkforceRequest) => client.workforce.respond({ body: input }),

        /** Suspends one active workforce request and includes the updated workforce projection. */
        suspend: (input: SuspendWorkforceRequest) => client.workforce.suspend({ body: input }),
      },
    }
  },
})
