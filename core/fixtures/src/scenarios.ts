import type { SessionHistoryMessage } from "@goddard-ai/session/schema"

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
    activePrompt: "prompt-tab-restore",
  },
  tools: {
    blockedPermission: "tool-write-query-injection",
  },
  acpUpdates: {
    session: fixtureSessionId("acp_update_matrix"),
    completedTurn: "turn-acp-updates-completed",
    activeTurn: "turn-acp-updates-active",
    prompt: "prompt-acp-updates",
    activePrompt: "prompt-acp-updates-active",
    permission: "permission-acp-updates-edit",
    readTool: "tool-acp-read",
    editTool: "tool-acp-edit",
    executeTool: "tool-acp-execute",
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
    lastSessionActivityAt: fixtureNow - 4 * 60_000,
    status: "blocked",
    title: "Add query injection for launchable states",
  })
  const activeSession = createFixtureSession({
    id: launchableStateIds.sessions.active,
    contextUsage: {
      size: 200_000,
      used: 96_000,
    },
    initiative: "Stabilize the app shell",
    lastAgentMessage: "Running focused checks around workbench tab restoration.",
    lastSessionActivityAt: fixtureNow - 16 * 60_000,
    status: "active",
    title: "Investigate tab restore race",
  })
  const errorSession = createFixtureSession({
    id: launchableStateIds.sessions.error,
    errorMessage: "Adapter exited before sending an initialize response.",
    lastAgentMessage: "The runtime failed during launch.",
    lastSessionActivityAt: fixtureNow - 2 * 60 * 60_000,
    status: "error",
    title: "Launch failed in agent adapter",
  })
  const completedSession = createFixtureSession({
    id: launchableStateIds.sessions.completed,
    activeDaemonSession: false,
    connectionMode: "history",
    lastAgentMessage: "Query cache tests are passing locally.",
    lastSessionActivityAt: fixtureNow - 5 * 60 * 60_000,
    status: "done",
    stopReason: "end_turn",
    title: "Finish launchable-state spike",
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
      mergeTargetBranch: null,
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

export function createActiveSessionScenario(
  session = createSessionTriageQueueScenario().activeSession,
) {
  const sessionResponse = createGetSessionResponse(session)
  const historyResponse = createSessionHistoryResponse({
    session,
    turns: [
      {
        inboxHeadline: "Agent finished a pass and is ready for direction.",
        inboxScope: "App shell",
        messages: [
          createSessionPromptMessage({
            requestId: launchableStateIds.requests.activePrompt,
            session,
            text: "Investigate the tab restore race in the app shell.",
          }),
          createSessionAgentChunkMessage({
            session,
            text: "I checked the restore path and am ready for the next direction.",
          }),
        ],
        promptRequestId: launchableStateIds.requests.activePrompt,
        startedAt: fixtureTimestamp,
        turnId: launchableStateIds.turns.active,
      },
    ],
  })
  const worktreeResponse = createSessionWorktreeResponse({
    session,
    worktree: {
      branchName: "codex/tab-restore",
      effectiveCwd: fixtureProjectPath,
      poweredBy: "goddard",
      repoRoot: fixtureProjectPath,
      requestedCwd: fixtureProjectPath,
      worktreeDir: "/Users/alec/.codex/worktrees/tab-restore/goddard-ai",
    },
  })

  return {
    session,
    sessionResponse,
    historyResponse,
    worktreeResponse,
  }
}

export function createAcpSessionUpdateMatrixScenario() {
  const session = createFixtureSession({
    id: launchableStateIds.acpUpdates.session,
    activeDaemonSession: true,
    agent: "codex",
    agentName: "Codex",
    availableCommands: [
      {
        name: "plan",
        description: "Create or revise the current plan",
      },
      {
        name: "review",
        description: "Review the current diff",
        input: {
          hint: "path or topic",
        },
      },
    ],
    configOptions: [
      {
        id: "model",
        type: "select",
        name: "Model",
        category: "model",
        currentValue: "gpt-5.1",
        options: [
          { value: "gpt-5.1", name: "GPT-5.1" },
          { value: "gpt-5.1-codex", name: "GPT-5.1 Codex" },
        ],
      },
      {
        id: "thought_level",
        type: "select",
        name: "Thought level",
        category: "thought_level",
        currentValue: "high",
        options: [
          { value: "medium", name: "Medium" },
          { value: "high", name: "High" },
        ],
      },
    ],
    connectionMode: "live",
    contextUsage: {
      size: 200_000,
      used: 84_200,
    },
    initiative: "Exercise the full ACP session/update surface",
    lastAgentMessage: "Streaming the active-turn branch after completing the update matrix.",
    lastSessionActivityAt: fixtureNow - 2 * 60_000,
    metadata: {
      launchableState: "acpUpdateMatrix",
      mock: true,
    },
    status: "active",
    title: "ACP session/update matrix",
  })

  const completedMessages = sequenceSessionMessages([
    createSessionPromptMessage({
      requestId: launchableStateIds.acpUpdates.prompt,
      session,
      text: "Exercise every ACP session/update variant in one transcript.",
    }).message,
    sessionUpdate(session, {
      sessionUpdate: "user_message_chunk",
      content: { type: "text", text: "Exercise every ACP session/update variant." },
    }),
    sessionUpdate(session, {
      sessionUpdate: "available_commands_update",
      availableCommands: session.availableCommands,
    }),
    sessionUpdate(session, {
      sessionUpdate: "current_mode_update",
      currentModeId: "build",
      availableModes: [
        { id: "ask", name: "Ask" },
        { id: "build", name: "Build", description: "Make code changes" },
      ],
    }),
    sessionUpdate(session, {
      sessionUpdate: "config_option_update",
      configOptions: session.configOptions,
    }),
    sessionUpdate(session, {
      sessionUpdate: "session_info_update",
      title: "ACP session/update matrix",
      updatedAt: new Date(fixtureNow - 3 * 60_000).toISOString(),
      _meta: {
        fixture: "acp-update-matrix",
      },
    }),
    sessionUpdate(session, {
      sessionUpdate: "usage_update",
      size: 200_000,
      used: 84_200,
    }),
    sessionUpdate(session, {
      sessionUpdate: "plan",
      entries: [
        { content: "Send route-only updates", priority: "high", status: "completed" },
        { content: "Run representative tools", priority: "high", status: "in_progress" },
        { content: "Leave a live active turn", priority: "medium", status: "pending" },
      ],
    }),
    sessionUpdate(session, {
      sessionUpdate: "agent_thought_chunk",
      content: { type: "text", text: "Mapped the update variants before rendering. " },
    }),
    sessionUpdate(session, {
      sessionUpdate: "tool_call",
      toolCallId: launchableStateIds.acpUpdates.readTool,
      title: "Read launch state registration",
      kind: "read",
      status: "in_progress",
      locations: [{ path: `${fixtureProjectPath}/app/src/dev/install.ts`, line: 66 }],
      rawInput: { path: `${fixtureProjectPath}/app/src/dev/install.ts` },
    }),
    sessionUpdate(session, {
      sessionUpdate: "tool_call_update",
      toolCallId: launchableStateIds.acpUpdates.readTool,
      status: "completed",
      content: [
        {
          type: "content",
          content: [{ type: "text", text: "Found defineLaunchableStates." }],
        },
      ],
    }),
    sessionUpdate(session, {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "The fixture covers route-only updates, " },
    }),
    sessionUpdate(session, {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "work updates, and visible text chunks." },
    }),
    sessionUpdate(session, {
      sessionUpdate: "agent_thought_chunk",
      content: { type: "text", text: "Preparing the edit tool call. " },
    }),
    sessionUpdate(session, {
      sessionUpdate: "tool_call",
      toolCallId: launchableStateIds.acpUpdates.editTool,
      title: "Patch fixture scenario",
      kind: "edit",
      status: "pending",
      rawInput: {
        path: `${fixtureProjectPath}/core/fixtures/src/scenarios.ts`,
      },
      locations: [{ path: `${fixtureProjectPath}/core/fixtures/src/scenarios.ts`, line: 202 }],
      content: [
        {
          type: "diff",
          path: `${fixtureProjectPath}/core/fixtures/src/scenarios.ts`,
          oldText: "export function createBlockedSessionScenario",
          newText: "export function createAcpSessionUpdateMatrixScenario",
        },
      ],
    }),
    permissionRequest(session),
    permissionResponse(),
    sessionUpdate(session, {
      sessionUpdate: "tool_call_update",
      toolCallId: launchableStateIds.acpUpdates.editTool,
      status: "completed",
    }),
    sessionUpdate(session, {
      sessionUpdate: "tool_call",
      toolCallId: launchableStateIds.acpUpdates.executeTool,
      title: "Run focused transcript checks",
      kind: "execute",
      status: "in_progress",
      rawInput: "pnpm --dir app run test -- session-chat",
      content: [{ type: "terminal", terminalId: "term-acp-update-matrix" }],
    }),
    sessionUpdate(session, {
      sessionUpdate: "tool_call_update",
      toolCallId: launchableStateIds.acpUpdates.executeTool,
      status: "failed",
      rawOutput: "Fixture intentionally captures a failed tool status.",
    }),
    sessionUpdate(session, {
      sessionUpdate: "plan",
      entries: [
        { content: "Send route-only updates", priority: "high", status: "completed" },
        { content: "Run representative tools", priority: "high", status: "completed" },
        { content: "Leave a live active turn", priority: "medium", status: "completed" },
      ],
    }),
    promptResult(launchableStateIds.acpUpdates.prompt, "end_turn"),
  ])

  const activeMessages = sequenceSessionMessages([
    createSessionPromptMessage({
      requestId: launchableStateIds.acpUpdates.activePrompt,
      session,
      text: "Keep one turn active while exercising streaming updates.",
    }).message,
    sessionUpdate(session, {
      sessionUpdate: "agent_thought_chunk",
      content: { type: "text", text: "Watching the active turn remain open. " },
    }),
    sessionUpdate(session, {
      sessionUpdate: "tool_call",
      toolCallId: "tool-acp-active-search",
      title: "Search active turn handlers",
      kind: "search",
      status: "in_progress",
      rawInput: { pattern: "sessionUpdate" },
      locations: [{ path: `${fixtureProjectPath}/app/src/session-chat/model.ts`, line: 183 }],
    }),
    sessionUpdate(session, {
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "This turn stays active with an in-progress search." },
    }),
  ])

  const sessionResponse = createGetSessionResponse(session)
  const historyResponse = createSessionHistoryResponse({
    session,
    turns: [
      {
        completedAt: new Date(fixtureNow - 3 * 60_000).toISOString(),
        completionKind: "result",
        messages: completedMessages,
        promptRequestId: launchableStateIds.acpUpdates.prompt,
        startedAt: new Date(fixtureNow - 5 * 60_000).toISOString(),
        stopReason: "end_turn",
        turnId: launchableStateIds.acpUpdates.completedTurn,
      },
      {
        completedAt: null,
        completionKind: null,
        messages: activeMessages,
        promptRequestId: launchableStateIds.acpUpdates.activePrompt,
        startedAt: new Date(fixtureNow - 2 * 60_000).toISOString(),
        stopReason: null,
        turnId: launchableStateIds.acpUpdates.activeTurn,
      },
    ],
  })

  return {
    historyResponse,
    session,
    sessionResponse,
  }
}

function sequenceSessionMessages(messages: unknown[]): SessionHistoryMessage[] {
  return messages.map((message, sequence) => ({
    message,
    sequence,
    sequenceStart: sequence,
  })) as SessionHistoryMessage[]
}

function sessionUpdate(
  session: ReturnType<typeof createFixtureSession>,
  update: Record<string, unknown>,
) {
  return {
    jsonrpc: "2.0",
    method: "session/update",
    params: {
      sessionId: session.acpSessionId,
      update,
    },
  }
}

function permissionRequest(session: ReturnType<typeof createFixtureSession>) {
  return {
    jsonrpc: "2.0",
    id: launchableStateIds.acpUpdates.permission,
    method: "session/request_permission",
    params: {
      options: [
        { kind: "allow_once", name: "Allow once", optionId: "allow_once" },
        { kind: "reject_once", name: "Reject", optionId: "reject_once" },
      ],
      sessionId: session.acpSessionId,
      toolCall: {
        kind: "edit",
        locations: [{ line: 202, path: `${fixtureProjectPath}/core/fixtures/src/scenarios.ts` }],
        rawInput: {
          path: `${fixtureProjectPath}/core/fixtures/src/scenarios.ts`,
        },
        status: "pending",
        title: "Approve fixture edit",
        toolCallId: launchableStateIds.acpUpdates.editTool,
      },
    },
  }
}

function permissionResponse() {
  return {
    jsonrpc: "2.0",
    id: launchableStateIds.acpUpdates.permission,
    result: {
      outcome: {
        outcome: "selected",
        optionId: "allow_once",
      },
    },
  }
}

function promptResult(requestId: string, stopReason: string) {
  return {
    jsonrpc: "2.0",
    id: requestId,
    result: {
      stopReason,
    },
  }
}
