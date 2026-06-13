import { fixtureInboxItemId, fixturePullRequestId, fixtureSessionId } from "./ids.ts"
import { createFixtureInboxItem, createListInboxResponse } from "./inbox.ts"
import { createFixturePullRequest, createGetPullRequestResponse } from "./pull-requests.ts"
import {
  createFixtureSession,
  createGetSessionResponse,
  createListSessionsResponse,
  createSessionAgentChunkMessage,
  createSessionChangesResponse,
  createSessionHistoryResponse,
  createSessionPermissionRequestMessage,
  createSessionPromptMessage,
  createSessionWorktreeResponse,
} from "./sessions.ts"
import { fixtureNow, fixtureProjectPath, fixtureTimestamp } from "./time.ts"

export const launchableStateIds = {
  sessions: {
    blocked: fixtureSessionId("launch_blocked"),
    active: fixtureSessionId("launch_active"),
    error: fixtureSessionId("launch_error"),
    completed: fixtureSessionId("launch_done"),
  },
  inbox: {
    blocked: fixtureInboxItemId("launch_blocked"),
    pullRequest: fixtureInboxItemId("launch_pr"),
    active: fixtureInboxItemId("launch_active"),
  },
  pullRequests: {
    review: fixturePullRequestId("launch_review"),
  },
  turns: {
    blocked: "turn-blocked",
    active: "turn-active",
  },
  requests: {
    blockedPrompt: "prompt-query-injection",
    blockedPermission: "permission-write-tests",
  },
  tools: {
    blockedPermission: "tool-write-query-injection",
  },
} as const

export function createSessionTriageQueueScenario() {
  const blockedSession = createFixtureSession({
    id: launchableStateIds.sessions.blocked,
    blockedReason: "Needs approval to edit the shared query cache.",
    contextUsage: {
      size: 200_000,
      used: 172_000,
    },
    inboxScope: "Query cache",
    lastAgentMessage:
      "I found the right seam, but need approval before changing shared cache behavior.",
    status: "blocked",
    title: "Add query injection for launchable states",
    updatedAt: fixtureNow - 4 * 60_000,
  })
  const activeSession = createFixtureSession({
    id: launchableStateIds.sessions.active,
    contextUsage: {
      size: 200_000,
      used: 96_000,
    },
    initiative: "Stabilize the app shell",
    lastAgentMessage: "Running focused checks around workbench tab restoration.",
    status: "active",
    title: "Investigate tab restore race",
    updatedAt: fixtureNow - 16 * 60_000,
  })
  const errorSession = createFixtureSession({
    id: launchableStateIds.sessions.error,
    errorMessage: "Adapter exited before sending an initialize response.",
    lastAgentMessage: "The runtime failed during launch.",
    status: "error",
    title: "Launch failed in agent adapter",
    updatedAt: fixtureNow - 2 * 60 * 60_000,
  })
  const completedSession = createFixtureSession({
    id: launchableStateIds.sessions.completed,
    activeDaemonSession: false,
    connectionMode: "history",
    lastAgentMessage: "Query cache tests are passing locally.",
    status: "done",
    stopReason: "end_turn",
    title: "Finish launchable-state spike",
    updatedAt: fixtureNow - 5 * 60 * 60_000,
  })
  const sessions = [blockedSession, activeSession, errorSession, completedSession]

  return {
    blockedSession,
    activeSession,
    errorSession,
    completedSession,
    response: createListSessionsResponse(sessions),
  }
}

export function createInboxAttentionQueueScenario(
  input: {
    blockedSession?: ReturnType<typeof createFixtureSession>
    activeSession?: ReturnType<typeof createFixtureSession>
    pullRequest?: ReturnType<typeof createFixturePullRequest>
  } = {},
) {
  const blockedSession =
    input.blockedSession ?? createFixtureSession({ id: launchableStateIds.sessions.blocked })
  const activeSession =
    input.activeSession ?? createFixtureSession({ id: launchableStateIds.sessions.active })
  const pullRequest =
    input.pullRequest ??
    createFixturePullRequest({
      id: launchableStateIds.pullRequests.review,
      prNumber: 128,
      updatedAt: fixtureNow - 8 * 60_000,
    })
  const response = createListInboxResponse([
    createFixtureInboxItem({
      id: launchableStateIds.inbox.blocked,
      entityId: blockedSession.id,
      headline: "Approve the shared query cache edit.",
      reason: "session.blocked",
      scope: "Query cache",
      turnId: launchableStateIds.turns.blocked,
      updatedAt: fixtureNow - 4 * 60_000,
    }),
    createFixtureInboxItem({
      id: launchableStateIds.inbox.pullRequest,
      entityId: pullRequest.id,
      headline: "Review comments landed on the launchable-state PR.",
      readAt: fixtureNow - 8 * 60_000,
      reason: "pull_request.updated",
      scope: "Pull request",
      status: "read",
      updatedAt: fixtureNow - 8 * 60_000,
    }),
    createFixtureInboxItem({
      id: launchableStateIds.inbox.active,
      entityId: activeSession.id,
      headline: "Agent finished a pass and is ready for direction.",
      reason: "session.turn_ended",
      scope: "App shell",
      turnId: launchableStateIds.turns.active,
      updatedAt: fixtureNow - 16 * 60_000,
    }),
  ])

  return {
    activeSession,
    blockedSession,
    pullRequest,
    pullRequestResponse: createGetPullRequestResponse(pullRequest),
    response,
  }
}

export function createBlockedSessionScenario(
  session = createSessionTriageQueueScenario().blockedSession,
) {
  const sessionResponse = createGetSessionResponse(session)
  const historyResponse = createSessionHistoryResponse({
    session,
    turns: [
      {
        inboxHeadline: "Approve the shared query cache edit.",
        inboxScope: "Query cache",
        messages: [
          createSessionPromptMessage({
            requestId: launchableStateIds.requests.blockedPrompt,
            session,
            text: "Add launchable states for the critical app review scenarios.",
          }),
          createSessionAgentChunkMessage({
            session,
            text: "I can seed the query cache for inbox, session list, and detail views.",
          }),
          createSessionPermissionRequestMessage({
            requestId: launchableStateIds.requests.blockedPermission,
            session,
            title: "Edit query cache injection",
            toolCallId: launchableStateIds.tools.blockedPermission,
          }),
        ],
        promptRequestId: launchableStateIds.requests.blockedPrompt,
        startedAt: fixtureTimestamp,
        turnId: launchableStateIds.turns.blocked,
      },
    ],
  })
  const worktreeResponse = createSessionWorktreeResponse({
    session,
    worktree: {
      branchName: "codex/dev-states",
      effectiveCwd: fixtureProjectPath,
      poweredBy: "goddard",
      repoRoot: fixtureProjectPath,
      requestedCwd: fixtureProjectPath,
      worktreeDir: "/Users/alec/.codex/worktrees/dev-states/goddard-ai",
    },
  })
  const changesResponse = createSessionChangesResponse({
    session,
    diff: [
      "diff --git a/app/src/lib/query.ts b/app/src/lib/query.ts",
      "index 34a4c7b..a91f3bc 100644",
      "--- a/app/src/lib/query.ts",
      "+++ b/app/src/lib/query.ts",
      "@@ -123,6 +123,10 @@ export class QueryClient {",
      "+  injectData(queryFn, args, data) {",
      "+    // Temporarily seed dev-only scenario data.",
      "+  }",
    ].join("\n"),
    workspaceRoot: "/Users/alec/.codex/worktrees/dev-states/goddard-ai",
  })

  return {
    session,
    sessionResponse,
    historyResponse,
    worktreeResponse,
    changesResponse,
  }
}
