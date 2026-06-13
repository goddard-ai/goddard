import type {
  DaemonSession,
  GetSessionChangesResponse,
  GetSessionHistoryResponse,
  ListSessionsResponse,
  SessionHistoryMessage,
} from "@goddard-ai/sdk"
import type { ListInboxResponse } from "@goddard-ai/inbox/schema"

import type { goddardSdk } from "~/sdk.ts"

type AppSdk = typeof goddardSdk
type GetPullRequestResponse = Awaited<ReturnType<AppSdk["pr"]["get"]>>
type GetSessionResponse = Awaited<ReturnType<AppSdk["session"]["get"]>>
type GetSessionWorktreeResponse = Awaited<ReturnType<AppSdk["session"]["worktree"]["get"]>>

const now = 1_780_310_400_000
const projectPath = "/Users/alec/Projects/goddard-ai"

function createSession(
  id: DaemonSession["id"],
  input: Partial<DaemonSession> & Pick<DaemonSession, "status" | "title">,
) {
  const { status, title, ...overrides } = input

  return {
    id,
    acpSessionId: `${id}_acp`,
    activeDaemonSession: status === "active" || status === "blocked",
    agent: "codex",
    agentName: "Codex",
    availableCommands: [],
    blockedReason: null,
    completedHidden: false,
    configOptions: [],
    connectionMode: status === "done" ? "history" : "live",
    contextUsage: null,
    createdAt: now - 86_400_000,
    cwd: projectPath,
    errorMessage: null,
    inboxScope: null,
    initiative: null,
    lastAgentMessage: null,
    mcpServers: [],
    metadata: null,
    models: null,
    permissions: null,
    prNumber: null,
    repository: "goddard-ai",
    status,
    stopReason: null,
    supportsLoadSession: true,
    title,
    titleState: "generated",
    token: null,
    updatedAt: now,
    ...overrides,
  } satisfies DaemonSession
}

function createPromptMessage(
  session: DaemonSession,
  requestId: string,
  text: string,
) {
  return {
    jsonrpc: "2.0",
    id: requestId,
    method: "session/prompt",
    params: {
      prompt: [{ text, type: "text" }],
      sessionId: session.acpSessionId,
    },
  } satisfies SessionHistoryMessage
}

function createAgentChunkMessage(session: DaemonSession, text: string) {
  return {
    jsonrpc: "2.0",
    method: "session/update",
    params: {
      sessionId: session.acpSessionId,
      update: {
        content: { text, type: "text" },
        sessionUpdate: "agent_message_chunk",
      },
    },
  } satisfies SessionHistoryMessage
}

function createPermissionRequestMessage(session: DaemonSession) {
  return {
    jsonrpc: "2.0",
    id: "permission-write-tests",
    method: "session/request_permission",
    params: {
      options: [
        { kind: "allow_once", name: "Allow once", optionId: "allow_once" },
        { kind: "reject_once", name: "Reject", optionId: "reject" },
      ],
      sessionId: session.acpSessionId,
      toolCall: {
        kind: "edit",
        locations: [{ line: 128, path: "app/src/lib/query.ts" }],
        status: "pending",
        title: "Edit query cache injection",
        toolCallId: "tool-write-query-injection",
      },
    },
  } satisfies SessionHistoryMessage
}

export const blockedSession = createSession("ses_launch_blocked" as DaemonSession["id"], {
  blockedReason: "Needs approval to edit the shared query cache.",
  contextUsage: {
    size: 200_000,
    used: 172_000,
  },
  inboxScope: "Query cache",
  lastAgentMessage: "I found the right seam, but need approval before changing shared cache behavior.",
  status: "blocked",
  title: "Add query injection for launchable states",
  updatedAt: now - 4 * 60_000,
})

export const activeSession = createSession("ses_launch_active" as DaemonSession["id"], {
  contextUsage: {
    size: 200_000,
    used: 96_000,
  },
  initiative: "Stabilize the app shell",
  lastAgentMessage: "Running focused checks around workbench tab restoration.",
  status: "active",
  title: "Investigate tab restore race",
  updatedAt: now - 16 * 60_000,
})

export const errorSession = createSession("ses_launch_error" as DaemonSession["id"], {
  errorMessage: "Adapter exited before sending an initialize response.",
  lastAgentMessage: "The runtime failed during launch.",
  status: "error",
  title: "Launch failed in agent adapter",
  updatedAt: now - 2 * 60 * 60_000,
})

export const completedSession = createSession("ses_launch_done" as DaemonSession["id"], {
  activeDaemonSession: false,
  connectionMode: "history",
  lastAgentMessage: "Query cache tests are passing locally.",
  status: "done",
  stopReason: "end_turn",
  title: "Finish launchable-state spike",
  updatedAt: now - 5 * 60 * 60_000,
})

export const criticalSessionsResponse = {
  hasMore: false,
  nextCursor: null,
  sessions: [blockedSession, activeSession, errorSession, completedSession],
} satisfies ListSessionsResponse

export const inboxAttentionResponse = {
  hasMore: false,
  nextCursor: null,
  items: [
    {
      id: "inb_launch_blocked" as const,
      entityId: blockedSession.id,
      headline: "Approve the shared query cache edit.",
      priority: "normal",
      readAt: null,
      reason: "session.blocked",
      scope: "Query cache",
      status: "unread",
      turnId: "turn-blocked",
      updatedAt: now - 4 * 60_000,
    },
    {
      id: "inb_launch_pr" as const,
      entityId: "pr_launch_review" as const,
      headline: "Review comments landed on the launchable-state PR.",
      priority: "normal",
      readAt: now - 8 * 60_000,
      reason: "pull_request.updated",
      scope: "Pull request",
      status: "read",
      turnId: null,
      updatedAt: now - 8 * 60_000,
    },
    {
      id: "inb_launch_active" as const,
      entityId: activeSession.id,
      headline: "Agent finished a pass and is ready for direction.",
      priority: "normal",
      readAt: null,
      reason: "session.turn_ended",
      scope: "App shell",
      status: "unread",
      turnId: "turn-active",
      updatedAt: now - 16 * 60_000,
    },
  ],
} satisfies ListInboxResponse

export const reviewPullRequestResponse = {
  pullRequest: {
    id: "pr_launch_review",
    cwd: projectPath,
    host: "github",
    owner: "goddard-ai",
    prNumber: 128,
    repo: "goddard-ai",
    updatedAt: now - 8 * 60_000,
  },
} satisfies GetPullRequestResponse

export const blockedSessionResponse = {
  session: blockedSession,
} satisfies GetSessionResponse

export const blockedSessionHistoryResponse = {
  id: blockedSession.id,
  acpSessionId: blockedSession.acpSessionId,
  connection: {
    activeDaemonSession: true,
    mode: "live",
    reconnectable: true,
  },
  hasMore: false,
  nextCursor: null,
  turns: [
    {
      completedAt: null,
      completionKind: null,
      inboxHeadline: "Approve the shared query cache edit.",
      inboxScope: "Query cache",
      messages: [
        createPromptMessage(
          blockedSession,
          "prompt-query-injection",
          "Add launchable states for the critical app review scenarios.",
        ),
        createAgentChunkMessage(
          blockedSession,
          "I can seed the query cache for inbox, session list, and detail views.",
        ),
        createPermissionRequestMessage(blockedSession),
      ],
      promptRequestId: "prompt-query-injection",
      sequence: 1,
      startedAt: "2026-06-13T15:35:00.000Z",
      stopReason: null,
      turnId: "turn-blocked",
    },
  ],
} satisfies GetSessionHistoryResponse

export const blockedSessionWorktreeResponse = {
  id: blockedSession.id,
  acpSessionId: blockedSession.acpSessionId,
  worktree: {
    branchName: "codex/dev-states",
    effectiveCwd: projectPath,
    poweredBy: "goddard",
    repoRoot: projectPath,
    requestedCwd: projectPath,
    worktreeDir: "/Users/alec/.codex/worktrees/dev-states/goddard-ai",
  },
} satisfies GetSessionWorktreeResponse

export const blockedSessionChangesResponse = {
  id: blockedSession.id,
  acpSessionId: blockedSession.acpSessionId,
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
  hasChanges: true,
  workspaceRoot: "/Users/alec/.codex/worktrees/dev-states/goddard-ai",
} satisfies GetSessionChangesResponse
