import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse, SessionHistoryTurn } from "@goddard-ai/sdk"

import { SessionChat } from "./model.ts"

const QA_SESSION_ID = "ses_session-chat-qa" as DaemonSession["id"]
const QA_ACP_SESSION_ID = "acp-session-chat-qa"
const QA_PROJECT_PATH = "/Users/alec/Projects/session-chat-qa"
const QA_AGENT_NAME = "QA Agent"
const FIRST_OLDER_CURSOR = "qa-older-page-1"
const SECOND_OLDER_CURSOR = "qa-older-page-2"

type QaTurnInput = {
  completionKind?: SessionHistoryTurn["completionKind"]
  completedAt?: string | null
  messages: acp.AnyMessage[]
  promptRequestId: SessionHistoryTurn["promptRequestId"]
  sequence: number
  startedAt: string
  stopReason?: SessionHistoryTurn["stopReason"]
  turnId: string
}

type QaStatusCase = {
  description: string
  label: string
  session: DaemonSession
  history: GetSessionHistoryResponse
}

function qaTimestamp(minutes: number) {
  const date = new Date(Date.UTC(2026, 3, 14, 14, 0, 0, 0))
  date.setUTCMinutes(date.getUTCMinutes() + minutes)
  return date.toISOString()
}

function createQaSession(overrides: Partial<DaemonSession> = {}) {
  return {
    id: QA_SESSION_ID,
    acpSessionId: QA_ACP_SESSION_ID,
    status: "blocked",
    stopReason: null,
    agent: "qa-acp",
    agentName: QA_AGENT_NAME,
    cwd: QA_PROJECT_PATH,
    mcpServers: [],
    connectionMode: "live",
    supportsLoadSession: true,
    activeDaemonSession: false,
    token: null,
    permissions: null,
    title: "Session chat ACP QA",
    titleState: "generated",
    repository: "session-chat-qa",
    prNumber: null,
    metadata: null,
    createdAt: 1_776_176_400_000,
    updatedAt: 1_776_177_000_000,
    errorMessage: null,
    blockedReason: "Generated pending permission request",
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    models: null,
    availableCommands: [],
    ...overrides,
  } satisfies DaemonSession
}

function createHistoryPage(input: {
  hasMore: boolean
  nextCursor: string | null
  session?: DaemonSession
  turns: SessionHistoryTurn[]
}): GetSessionHistoryResponse {
  const session = input.session ?? createQaSession()

  return {
    id: session.id,
    acpSessionId: session.acpSessionId,
    connection: {
      activeDaemonSession: session.activeDaemonSession,
      mode: session.connectionMode === "none" ? "none" : session.connectionMode,
      reconnectable: session.connectionMode === "live" && !session.activeDaemonSession,
    },
    turns: input.turns,
    nextCursor: input.nextCursor,
    hasMore: input.hasMore,
  }
}

function createTurn(input: QaTurnInput) {
  return {
    turnId: input.turnId,
    sequence: input.sequence,
    promptRequestId: input.promptRequestId,
    startedAt: input.startedAt,
    completedAt:
      input.completedAt === undefined ? qaTimestamp(input.sequence * 7 + 4) : input.completedAt,
    completionKind: input.completionKind === undefined ? "result" : input.completionKind,
    stopReason: input.stopReason === undefined ? "end_turn" : input.stopReason,
    inboxScope: null,
    inboxHeadline: null,
    messages: input.messages,
  } satisfies SessionHistoryTurn
}

function promptMessage(id: string, text: string, extraBlocks: acp.ContentBlock[] = []) {
  return {
    jsonrpc: "2.0",
    id,
    method: acp.AGENT_METHODS.session_prompt,
    params: {
      sessionId: QA_ACP_SESSION_ID,
      prompt: [
        {
          type: "text",
          text,
        },
        ...extraBlocks,
      ],
    },
  } satisfies acp.AnyMessage
}

function promptResult(id: string, stopReason: SessionHistoryTurn["stopReason"] = "end_turn") {
  return {
    jsonrpc: "2.0",
    id,
    result: {
      stopReason,
    },
  } satisfies acp.AnyMessage
}

function promptError(id: string, message: string) {
  return {
    jsonrpc: "2.0",
    id,
    error: {
      code: -32_000,
      message,
    },
  } satisfies acp.AnyMessage
}

function agentChunk(text: string) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: QA_ACP_SESSION_ID,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: {
          type: "text",
          text,
        },
      },
    },
  } satisfies acp.AnyMessage
}

function toolCall(input: {
  content?: unknown[]
  kind: string
  locations?: unknown[]
  status?: string
  title: string
  toolCallId: string
}) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: QA_ACP_SESSION_ID,
      update: {
        sessionUpdate: "tool_call",
        toolCallId: input.toolCallId,
        title: input.title,
        kind: input.kind,
        status: input.status ?? "in_progress",
        content: input.content ?? [],
        locations: input.locations ?? [],
      },
    },
  } satisfies acp.AnyMessage
}

function toolCallUpdate(input: {
  content?: unknown[]
  locations?: unknown[]
  status: string
  title?: string
  toolCallId: string
}) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: QA_ACP_SESSION_ID,
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: input.toolCallId,
        title: input.title,
        status: input.status,
        content: input.content,
        locations: input.locations,
      },
    },
  } satisfies acp.AnyMessage
}

function planUpdate(entries: acp.Plan["entries"]) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: QA_ACP_SESSION_ID,
      update: {
        sessionUpdate: "plan",
        entries,
      },
    },
  } satisfies acp.AnyMessage
}

function permissionRequest(input: {
  id: string
  kind?: string
  rawInput?: unknown
  title: string
}) {
  return {
    jsonrpc: "2.0",
    id: input.id,
    method: acp.CLIENT_METHODS.session_request_permission,
    params: {
      sessionId: QA_ACP_SESSION_ID,
      options: [
        { optionId: "allow-once", name: "Allow once", kind: "allow_once" },
        { optionId: "allow-always", name: "Allow always", kind: "allow_always" },
        { optionId: "reject-once", name: "Reject once", kind: "reject_once" },
        { optionId: "reject-always", name: "Reject always", kind: "reject_always" },
      ],
      toolCall: {
        toolCallId: `${input.id}:tool`,
        title: input.title,
        kind: input.kind ?? "edit",
        status: "pending",
        rawInput: input.rawInput ?? {
          reason: "Generated manual QA permission request.",
          path: `${QA_PROJECT_PATH}/app/src/session-chat/manual-qa.ts`,
        },
        locations: [
          {
            path: `${QA_PROJECT_PATH}/app/src/session-chat/manual-qa.ts`,
            line: 42,
          },
        ],
      },
    },
  } satisfies acp.AnyMessage
}

function permissionResponse(
  id: string,
  outcome: { outcome: "cancelled" } | { outcome: "selected"; optionId: string },
) {
  return {
    jsonrpc: "2.0",
    id,
    result: {
      outcome,
    },
  } satisfies acp.AnyMessage
}

function permissionError(id: string, message: string) {
  return {
    jsonrpc: "2.0",
    id,
    error: {
      code: -32_003,
      message,
    },
  } satisfies acp.AnyMessage
}

export function createSessionChatQaPermissionResponse(input: {
  optionId: string
  requestId: string | number
}) {
  return permissionResponse(String(input.requestId), {
    outcome: "selected",
    optionId: input.optionId,
  })
}

function contentText(text: string) {
  return [
    {
      type: "content",
      content: [{ type: "text", text }],
    },
  ]
}

function buildInitialTurns() {
  return [
    createTurn({
      turnId: "qa-turn-plan-tools",
      sequence: 5,
      promptRequestId: "qa-prompt-plan-tools",
      startedAt: qaTimestamp(35),
      messages: [
        promptMessage("qa-prompt-plan-tools", "Generate representative transcript states."),
        planUpdate([
          { content: "Create deterministic ACP messages", priority: "high", status: "completed" },
          { content: "Render the transcript rows", priority: "high", status: "in_progress" },
          { content: "Check scroll anchoring", priority: "medium", status: "pending" },
        ]),
        planUpdate([
          { content: "Create deterministic ACP messages", priority: "high", status: "completed" },
          { content: "Render the transcript rows", priority: "high", status: "in_progress" },
          { content: "Check scroll anchoring", priority: "medium", status: "pending" },
        ]),
        agentChunk("I am generating the QA transcript from ACP session updates.\n\n"),
        agentChunk("This turn includes plan rows, streamed assistant text, and tool rows."),
        toolCall({
          toolCallId: "qa-tool-read",
          title: "Read transcript model",
          kind: "read",
          status: "completed",
          content: contentText("Read the session chat model and transcript row builders."),
          locations: [{ path: `${QA_PROJECT_PATH}/app/src/session-chat/model.ts`, line: 1 }],
        }),
        toolCall({
          toolCallId: "qa-tool-edit",
          title: "Patch transcript fixture",
          kind: "edit",
          status: "in_progress",
          content: [
            {
              type: "diff",
              path: `${QA_PROJECT_PATH}/app/src/session-chat/transcript-debug-data.ts`,
              oldText: "export const SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES = buildDebugMessages()",
              newText: "export const SESSION_CHAT_QA_INITIAL_HISTORY = buildSessionChatQaHistory()",
            },
          ],
          locations: [
            { path: `${QA_PROJECT_PATH}/app/src/session-chat/transcript-debug-data.ts`, line: 1 },
          ],
        }),
        toolCallUpdate({
          toolCallId: "qa-tool-edit",
          status: "completed",
          title: "Patch transcript fixture",
          content: contentText("Generated ACP-backed fixture data and updated the debug view."),
        }),
        toolCall({
          toolCallId: "qa-tool-terminal",
          title: "Run manual QA probe",
          kind: "execute",
          status: "failed",
          content: [{ type: "terminal", terminalId: "qa-terminal-1" }],
        }),
        toolCall({
          toolCallId: "qa-tool-pending",
          title: "Queue follow-up search",
          kind: "search",
          status: "pending",
          content: contentText("Pending generated tool row."),
        }),
        toolCall({
          toolCallId: "qa-tool-running",
          title: "Fetch generated reference",
          kind: "fetch",
          status: "in_progress",
          content: contentText("In-progress generated tool row."),
        }),
        promptResult("qa-prompt-plan-tools"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-permission-pending",
      sequence: 6,
      promptRequestId: "qa-prompt-permission-pending",
      startedAt: qaTimestamp(44),
      messages: [
        promptMessage("qa-prompt-permission-pending", "Leave this permission request pending."),
        agentChunk("I need permission before writing to the generated QA file."),
        permissionRequest({
          id: "qa-permission-pending",
          title: "Write generated QA file",
        }),
        promptResult("qa-prompt-permission-pending"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-permission-allowed",
      sequence: 7,
      promptRequestId: "qa-prompt-permission-allowed",
      startedAt: qaTimestamp(51),
      messages: [
        promptMessage("qa-prompt-permission-allowed", "Show an allowed permission request."),
        permissionRequest({
          id: "qa-permission-allowed",
          title: "Apply generated edit",
        }),
        permissionResponse("qa-permission-allowed", {
          outcome: "selected",
          optionId: "allow-once",
        }),
        promptResult("qa-prompt-permission-allowed"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-permission-denied",
      sequence: 8,
      promptRequestId: "qa-prompt-permission-denied",
      startedAt: qaTimestamp(58),
      messages: [
        promptMessage("qa-prompt-permission-denied", "Show denied and failed permission states."),
        permissionRequest({
          id: "qa-permission-denied",
          title: "Delete generated fixture",
          kind: "delete",
        }),
        permissionResponse("qa-permission-denied", {
          outcome: "selected",
          optionId: "reject-once",
        }),
        permissionRequest({
          id: "qa-permission-failed",
          title: "Move generated fixture",
          kind: "move",
        }),
        permissionError("qa-permission-failed", "The debug client rejected the response."),
        permissionRequest({
          id: "qa-permission-cancelled",
          title: "Fetch generated context",
          kind: "fetch",
        }),
        permissionResponse("qa-permission-cancelled", { outcome: "cancelled" }),
        permissionRequest({
          id: "qa-permission-resolved",
          title: "Switch generated mode",
          kind: "switch_mode",
        }),
        permissionResponse("qa-permission-resolved", {
          outcome: "selected",
          optionId: "unknown-option",
        }),
        promptResult("qa-prompt-permission-denied"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-interrupted",
      sequence: 9,
      promptRequestId: "qa-prompt-interrupted",
      startedAt: qaTimestamp(65),
      completedAt: null,
      completionKind: null,
      stopReason: null,
      messages: [
        promptMessage("qa-prompt-interrupted", "Show an interrupted turn stop row."),
        agentChunk("This turn intentionally has no terminal prompt response."),
      ],
    }),
  ]
}

function buildFirstOlderTurns() {
  return [
    createTurn({
      turnId: "qa-turn-completed",
      sequence: 2,
      promptRequestId: "qa-prompt-completed",
      startedAt: qaTimestamp(14),
      messages: [
        promptMessage(
          "qa-prompt-completed",
          "Show a completed prompt with a linked context attachment.",
          [
            {
              type: "resource_link",
              uri: "file:///Users/alec/Projects/session-chat-qa/README.md",
              name: "README.md",
              title: "QA fixture notes",
              description: "Generated resource link for transcript rendering.",
            },
          ],
        ),
        agentChunk("Completed turn content with a resource link prompt."),
        promptResult("qa-prompt-completed"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-stopped",
      sequence: 3,
      promptRequestId: "qa-prompt-stopped",
      startedAt: qaTimestamp(21),
      stopReason: "max_tokens",
      messages: [
        promptMessage("qa-prompt-stopped", "Show a stopped turn."),
        agentChunk("This generated turn reaches a stop reason before normal completion."),
        promptResult("qa-prompt-stopped", "max_tokens"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-failed",
      sequence: 4,
      promptRequestId: "qa-prompt-failed",
      startedAt: qaTimestamp(28),
      completionKind: "error",
      stopReason: null,
      messages: [
        promptMessage("qa-prompt-failed", "Show a failed turn."),
        agentChunk("The next frame is a prompt error so the failure row has a reason."),
        promptError("qa-prompt-failed", "Generated ACP failure for manual QA."),
      ],
    }),
  ]
}

function buildSecondOlderTurns() {
  return [
    createTurn({
      turnId: "qa-turn-oldest",
      sequence: 0,
      promptRequestId: "qa-prompt-oldest",
      startedAt: qaTimestamp(0),
      messages: [
        promptMessage("qa-prompt-oldest", "Oldest generated prompt."),
        agentChunk("This row appears only after loading the second older history page."),
        promptResult("qa-prompt-oldest"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-cancelled",
      sequence: 1,
      promptRequestId: "qa-prompt-cancelled",
      startedAt: qaTimestamp(7),
      stopReason: "cancelled",
      messages: [
        promptMessage("qa-prompt-cancelled", "Show a cancelled turn."),
        agentChunk("The prompt is cancelled to exercise cancellation wording."),
        promptResult("qa-prompt-cancelled", "cancelled"),
      ],
    }),
  ]
}

export function createSessionChatQaInitialHistory() {
  return createHistoryPage({
    turns: buildInitialTurns(),
    nextCursor: FIRST_OLDER_CURSOR,
    hasMore: true,
  })
}

export function findSessionChatQaOlderHistoryPage(cursor: string | null) {
  if (cursor === FIRST_OLDER_CURSOR) {
    return createHistoryPage({
      turns: buildFirstOlderTurns(),
      nextCursor: SECOND_OLDER_CURSOR,
      hasMore: true,
    })
  }

  if (cursor === SECOND_OLDER_CURSOR) {
    return createHistoryPage({
      turns: buildSecondOlderTurns(),
      nextCursor: null,
      hasMore: false,
    })
  }

  return null
}

export function createSessionChatQaChat() {
  return new SessionChat({
    history: createSessionChatQaInitialHistory(),
    session: createQaSession(),
  })
}

export function createSessionChatQaChatWithAllHistory() {
  const chat = createSessionChatQaChat()

  while (chat.nextCursor) {
    const page = findSessionChatQaOlderHistoryPage(chat.nextCursor)

    if (!page) {
      break
    }

    chat.prependOlderHistory(page)
  }

  return chat
}

export function createSessionChatQaStatusCases(): QaStatusCase[] {
  return [
    {
      label: "Ready",
      description: "No running turn; composer should be usable.",
      session: createQaSession({
        status: "active",
        activeDaemonSession: true,
        blockedReason: null,
      }),
      history: createHistoryPage({ turns: [], nextCursor: null, hasMore: false }),
    },
    {
      label: "Running",
      description: "Active daemon session with a running turn; stop should be available.",
      session: createQaSession({
        status: "active",
        activeDaemonSession: true,
        blockedReason: null,
      }),
      history: createHistoryPage({
        turns: [
          createTurn({
            turnId: "qa-status-running",
            sequence: 0,
            promptRequestId: "qa-status-prompt-running",
            startedAt: qaTimestamp(0),
            completedAt: null,
            completionKind: null,
            stopReason: null,
            messages: [
              promptMessage("qa-status-prompt-running", "Running status case."),
              agentChunk("Still working."),
            ],
          }),
        ],
        nextCursor: null,
        hasMore: false,
      }),
    },
    {
      label: "Blocked",
      description: "Pending permission request; permission controls should be visible.",
      session: createQaSession(),
      history: createSessionChatQaInitialHistory(),
    },
    {
      label: "Reconnectable",
      description: "Live-capable session without an active daemon; reconnect should be available.",
      session: createQaSession({ status: "active", blockedReason: null }),
      history: createHistoryPage({
        turns: buildInitialTurns().slice(0, 1),
        nextCursor: null,
        hasMore: false,
      }),
    },
    {
      label: "Completed",
      description: "Done session; only passive transcript actions should remain.",
      session: createQaSession({
        status: "done",
        activeDaemonSession: false,
        blockedReason: null,
        lastAgentMessage: "Generated completed status summary.",
      }),
      history: createHistoryPage({
        turns: buildFirstOlderTurns().slice(0, 1),
        nextCursor: null,
        hasMore: false,
      }),
    },
    {
      label: "Failed",
      description: "Session error state with visible failure summary.",
      session: createQaSession({
        status: "error",
        activeDaemonSession: false,
        blockedReason: null,
        errorMessage: "Generated session failure.",
      }),
      history: createHistoryPage({
        turns: buildFirstOlderTurns().slice(2),
        nextCursor: null,
        hasMore: false,
      }),
    },
    {
      label: "Cancelled",
      description: "Cancelled session status and cancellation stop row.",
      session: createQaSession({
        status: "cancelled",
        activeDaemonSession: false,
        blockedReason: null,
      }),
      history: createHistoryPage({
        turns: buildSecondOlderTurns().slice(1),
        nextCursor: null,
        hasMore: false,
      }),
    },
  ]
}

export function createSessionChatQaStatusSummary() {
  return createSessionChatQaStatusCases().map((statusCase) => {
    const chat = new SessionChat({
      history: statusCase.history,
      session: statusCase.session,
    })
    const canStop = chat.summary.activeTurnId !== null && chat.session.activeDaemonSession
    const canReconnect =
      chat.connection.reconnectable &&
      !chat.session.activeDaemonSession &&
      chat.session.connectionMode === "live"

    return {
      label: statusCase.label,
      description: statusCase.description,
      status: chat.summary.status,
      actions: [
        chat.session.cwd.trim().length > 0 ? "changes" : null,
        canStop ? "stop" : null,
        canReconnect ? "reconnect" : null,
      ].filter(Boolean) as string[],
    }
  })
}
