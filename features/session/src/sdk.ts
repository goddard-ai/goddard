import type * as acp from "@agentclientprotocol/sdk"
import type {
  CancelSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest,
  GetSessionHistoryRequest,
  ListSessionsRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionLaunchPreviewRequest,
  SessionSubpackagesRequest,
  SteerSessionRequest,
} from "@goddard-ai/schema/daemon"
import type { DaemonSessionIdParams } from "@goddard-ai/schema/id"
import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { sessionIpcRoutes } from "./daemon-ipc.ts"
import type {
  GetSessionWorktreeRequest,
  MountSessionReviewSessionRequest,
  RunSessionReviewSessionRequest,
  UnmountSessionReviewSessionRequest,
} from "./schema.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
  wrap({ client }) {
    return {
      session: {
        /** Creates one daemon-managed session record. */
        create: async (input: CreateSessionRequest) => client.session.create({ body: input }),

        /** Lists daemon-managed sessions and pagination state. */
        list: async (input: ListSessionsRequest) => client.session.list({ body: input }),

        /** Fetches one daemon-managed session record. */
        get: async (input: DaemonSessionIdParams) => client.session.get({ body: input }),

        /** Reconnects to one daemon-managed session record. */
        connect: async (input: DaemonSessionIdParams) => client.session.connect({ body: input }),

        /** Reads one daemon-managed session history with session identity and connection state. */
        history: async (input: GetSessionHistoryRequest) => client.session.history({ body: input }),

        /** Reads the current git diff for one daemon-managed session workspace. */
        changes: async (input: GetSessionChangesRequest) => client.session.changes({ body: input }),

        /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
        composerSuggestions: async (input: SessionComposerSuggestionsRequest) =>
          client.session.composerSuggestions({ body: input }),

        /** Reads draft composer suggestions that only depend on one repository cwd. */
        draftSuggestions: async (input: SessionDraftSuggestionsRequest) =>
          client.session.draftSuggestions({ body: input }),

        /** Loads launch-time adapter and repository capabilities before a session is created. */
        launchPreview: async (input: SessionLaunchPreviewRequest) =>
          client.session.launchPreview({ body: input }),

        /** Discovers launchable subpackage working directories under one project cwd. */
        subpackages: async (input: SessionSubpackagesRequest) =>
          client.session.subpackages({ body: input }),

        /** Reads one daemon-managed session diagnostics with event history and connection state. */
        diagnostics: async (input: DaemonSessionIdParams) =>
          client.session.diagnostics({ body: input }),

        /** Reads persisted worktree metadata attached to one daemon-managed session. */
        worktree: (input: GetSessionWorktreeRequest) =>
          client.session.worktree.get({ body: input }),

        /** Mounts a review session for one daemon-managed session worktree. */
        mountReviewSession: (input: MountSessionReviewSessionRequest) =>
          client.session.reviewSession.mount({ body: input }),

        /** Runs one mounted review session immediately. */
        runReviewSession: (input: RunSessionReviewSessionRequest) =>
          client.session.reviewSession.run({ body: input }),

        /** Unmounts a review session from one daemon-managed session worktree. */
        unmountReviewSession: (input: UnmountSessionReviewSessionRequest) =>
          client.session.reviewSession.unmount({ body: input }),

        /** Reads persisted workforce metadata attached to one daemon-managed session. */
        workforce: async (input: DaemonSessionIdParams) =>
          client.session.workforce.get({ body: input }),

        /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
        shutdown: async (input: DaemonSessionIdParams) => client.session.shutdown({ body: input }),

        /** Marks one session inbox row completed without shutting down the session. */
        complete: async (input: DaemonSessionIdParams) => client.session.complete({ body: input }),

        /** Records the current session initiative without creating an inbox row. */
        declareInitiative: async (input: DeclareSessionInitiativeRequest) =>
          client.session.declareInitiative({ body: input }),

        /** Reports a session blocker and marks the session inbox row unread. */
        reportBlocker: async (input: ReportSessionBlockerRequest) =>
          client.session.reportBlocker({ body: input }),

        /** Reports an end-of-turn session update when no other entity claimed attention. */
        reportTurnEnded: async (input: ReportSessionTurnEndedRequest) =>
          client.session.reportTurnEnded({ body: input }),

        /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
        cancel: async (input: CancelSessionRequest) => client.session.cancel({ body: input }),

        /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
        steer: async (input: SteerSessionRequest) => client.session.steer({ body: input }),

        /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
        send: async (input: SendSessionMessageRequest) => client.session.send({ body: input }),

        /** Subscribes to live daemon-published ACP messages for one daemon-managed session id. */
        subscribe: async (
          input: DaemonSessionIdParams,
          onMessage: (message: acp.AnyMessage) => void,
        ): Promise<() => void> => {
          const controller = new AbortController()
          const events = await client.session.messageEvents({
            query: input,
            signal: controller.signal,
          })
          void (async () => {
            for await (const payload of events) {
              if (controller.signal.aborted) {
                break
              }
              onMessage(payload.message)
            }
          })()
          return () => controller.abort()
        },

        /** Resolves one daemon session token to its daemon session id. */
        resolveToken: async (input: ResolveSessionTokenRequest) =>
          client.session.resolveToken({ body: input }),
      },
    }
  },
})
