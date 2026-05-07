import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse, SessionHistoryTurn } from "@goddard-ai/sdk"

import { SessionChat } from "./model.ts"

const DEBUG_SESSION_ID = "ses_session-chat-qa" as DaemonSession["id"]
const DEBUG_ACP_SESSION_ID = "acp-session-chat-qa"
const DEBUG_PROJECT_PATH = "/Users/alec/Projects/session-chat-qa"
const DEBUG_AGENT_NAME = "QA Agent"
const FIRST_OLDER_CURSOR = "qa-older-page-1"
const SECOND_OLDER_CURSOR = "qa-older-page-2"

type DebugTurnInput = {
  completionKind?: SessionHistoryTurn["completionKind"]
  completedAt?: string | null
  messages: acp.AnyMessage[]
  promptRequestId: SessionHistoryTurn["promptRequestId"]
  sequence: number
  startedAt: string
  stopReason?: SessionHistoryTurn["stopReason"]
  turnId: string
}

type DebugStatusCase = {
  description: string
  label: string
  session: DaemonSession
  history: GetSessionHistoryResponse
}

function createDebugTimestamp(minutes: number) {
  const date = new Date(Date.UTC(2026, 3, 14, 14, 0, 0, 0))
  date.setUTCMinutes(date.getUTCMinutes() + minutes)
  return date.toISOString()
}

function createDebugSession(overrides: Partial<DaemonSession> = {}) {
  return {
    id: DEBUG_SESSION_ID,
    acpSessionId: DEBUG_ACP_SESSION_ID,
    status: "blocked",
    stopReason: null,
    agent: "qa-acp",
    agentName: DEBUG_AGENT_NAME,
    cwd: DEBUG_PROJECT_PATH,
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
  const session = input.session ?? createDebugSession()

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

function createTurn(input: DebugTurnInput) {
  return {
    turnId: input.turnId,
    sequence: input.sequence,
    promptRequestId: input.promptRequestId,
    startedAt: input.startedAt,
    completedAt:
      input.completedAt === undefined
        ? createDebugTimestamp(input.sequence * 7 + 4)
        : input.completedAt,
    completionKind: input.completionKind === undefined ? "result" : input.completionKind,
    stopReason: input.stopReason === undefined ? "end_turn" : input.stopReason,
    inboxScope: null,
    inboxHeadline: null,
    messages: input.messages,
  } satisfies SessionHistoryTurn
}

function createPromptMessage(id: string, text: string, extraBlocks: acp.ContentBlock[] = []) {
  return {
    jsonrpc: "2.0",
    id,
    method: acp.AGENT_METHODS.session_prompt,
    params: {
      sessionId: DEBUG_ACP_SESSION_ID,
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

function createPromptResult(id: string, stopReason: SessionHistoryTurn["stopReason"] = "end_turn") {
  return {
    jsonrpc: "2.0",
    id,
    result: {
      stopReason,
    },
  } satisfies acp.AnyMessage
}

function createPromptError(id: string, message: string) {
  return {
    jsonrpc: "2.0",
    id,
    error: {
      code: -32_000,
      message,
    },
  } satisfies acp.AnyMessage
}

function createAgentChunk(text: string) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: DEBUG_ACP_SESSION_ID,
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

function createToolCall(input: {
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
      sessionId: DEBUG_ACP_SESSION_ID,
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

function createToolCallUpdate(input: {
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
      sessionId: DEBUG_ACP_SESSION_ID,
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

function createPlanUpdate(entries: acp.Plan["entries"]) {
  return {
    jsonrpc: "2.0",
    method: acp.CLIENT_METHODS.session_update,
    params: {
      sessionId: DEBUG_ACP_SESSION_ID,
      update: {
        sessionUpdate: "plan",
        entries,
      },
    },
  } satisfies acp.AnyMessage
}

function createPermissionRequest(input: {
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
      sessionId: DEBUG_ACP_SESSION_ID,
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
          path: `${DEBUG_PROJECT_PATH}/app/src/session-chat/manual-qa.ts`,
        },
        locations: [
          {
            path: `${DEBUG_PROJECT_PATH}/app/src/session-chat/manual-qa.ts`,
            line: 42,
          },
        ],
      },
    },
  } satisfies acp.AnyMessage
}

function createPermissionResponseMessage(
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

function createPermissionError(id: string, message: string) {
  return {
    jsonrpc: "2.0",
    id,
    error: {
      code: -32_003,
      message,
    },
  } satisfies acp.AnyMessage
}

function createSelectedPermissionResponse(input: { optionId: string; requestId: string | number }) {
  return createPermissionResponseMessage(String(input.requestId), {
    outcome: "selected",
    optionId: input.optionId,
  })
}

function createContentText(text: string) {
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
      startedAt: createDebugTimestamp(35),
      messages: [
        createPromptMessage("qa-prompt-plan-tools", "Generate representative transcript states."),
        createPlanUpdate([
          { content: "Create deterministic ACP messages", priority: "high", status: "completed" },
          { content: "Render the transcript rows", priority: "high", status: "in_progress" },
          { content: "Check scroll anchoring", priority: "medium", status: "pending" },
        ]),
        createPlanUpdate([
          { content: "Create deterministic ACP messages", priority: "high", status: "completed" },
          { content: "Render the transcript rows", priority: "high", status: "in_progress" },
          { content: "Check scroll anchoring", priority: "medium", status: "pending" },
        ]),
        createAgentChunk("I am generating the QA transcript from ACP session updates.\n\n"),
        createAgentChunk("This turn includes plan rows, streamed assistant text, and tool rows."),
        createToolCall({
          toolCallId: "qa-tool-read",
          title: "Read transcript model",
          kind: "read",
          status: "completed",
          content: createContentText("Read the session chat model and transcript row builders."),
          locations: [{ path: `${DEBUG_PROJECT_PATH}/app/src/session-chat/model.ts`, line: 1 }],
        }),
        createToolCall({
          toolCallId: "qa-tool-edit",
          title: "Patch transcript fixture",
          kind: "edit",
          status: "in_progress",
          content: [
            {
              type: "diff",
              path: `${DEBUG_PROJECT_PATH}/app/src/session-chat/transcript-debug-data.ts`,
              oldText: "export const SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES = buildDebugMessages()",
              newText: "const initialHistory = buildInitialHistory()",
            },
          ],
          locations: [
            {
              path: `${DEBUG_PROJECT_PATH}/app/src/session-chat/transcript-debug-data.ts`,
              line: 1,
            },
          ],
        }),
        createToolCallUpdate({
          toolCallId: "qa-tool-edit",
          status: "completed",
          title: "Patch transcript fixture",
          content: createContentText(
            "Generated ACP-backed fixture data and updated the debug view.",
          ),
        }),
        createToolCall({
          toolCallId: "qa-tool-terminal",
          title: "Run manual QA probe",
          kind: "execute",
          status: "failed",
          content: [{ type: "terminal", terminalId: "qa-terminal-1" }],
        }),
        createToolCall({
          toolCallId: "qa-tool-pending",
          title: "Queue follow-up search",
          kind: "search",
          status: "pending",
          content: createContentText("Pending generated tool row."),
        }),
        createToolCall({
          toolCallId: "qa-tool-running",
          title: "Fetch generated reference",
          kind: "fetch",
          status: "in_progress",
          content: createContentText("In-progress generated tool row."),
        }),
        createPromptResult("qa-prompt-plan-tools"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-permission-pending",
      sequence: 6,
      promptRequestId: "qa-prompt-permission-pending",
      startedAt: createDebugTimestamp(44),
      messages: [
        createPromptMessage(
          "qa-prompt-permission-pending",
          "Leave this permission request pending.",
        ),
        createAgentChunk("I need permission before writing to the generated QA file."),
        createPermissionRequest({
          id: "qa-permission-pending",
          title: "Write generated QA file",
        }),
        createPromptResult("qa-prompt-permission-pending"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-permission-allowed",
      sequence: 7,
      promptRequestId: "qa-prompt-permission-allowed",
      startedAt: createDebugTimestamp(51),
      messages: [
        createPromptMessage("qa-prompt-permission-allowed", "Show an allowed permission request."),
        createPermissionRequest({
          id: "qa-permission-allowed",
          title: "Apply generated edit",
        }),
        createPermissionResponseMessage("qa-permission-allowed", {
          outcome: "selected",
          optionId: "allow-once",
        }),
        createPromptResult("qa-prompt-permission-allowed"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-permission-denied",
      sequence: 8,
      promptRequestId: "qa-prompt-permission-denied",
      startedAt: createDebugTimestamp(58),
      messages: [
        createPromptMessage(
          "qa-prompt-permission-denied",
          "Show denied and failed permission states.",
        ),
        createPermissionRequest({
          id: "qa-permission-denied",
          title: "Delete generated fixture",
          kind: "delete",
        }),
        createPermissionResponseMessage("qa-permission-denied", {
          outcome: "selected",
          optionId: "reject-once",
        }),
        createPermissionRequest({
          id: "qa-permission-failed",
          title: "Move generated fixture",
          kind: "move",
        }),
        createPermissionError("qa-permission-failed", "The debug client rejected the response."),
        createPermissionRequest({
          id: "qa-permission-cancelled",
          title: "Fetch generated context",
          kind: "fetch",
        }),
        createPermissionResponseMessage("qa-permission-cancelled", { outcome: "cancelled" }),
        createPermissionRequest({
          id: "qa-permission-resolved",
          title: "Switch generated mode",
          kind: "switch_mode",
        }),
        createPermissionResponseMessage("qa-permission-resolved", {
          outcome: "selected",
          optionId: "unknown-option",
        }),
        createPromptResult("qa-prompt-permission-denied"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-interrupted",
      sequence: 9,
      promptRequestId: "qa-prompt-interrupted",
      startedAt: createDebugTimestamp(65),
      completedAt: null,
      completionKind: null,
      stopReason: null,
      messages: [
        createPromptMessage("qa-prompt-interrupted", "Show an interrupted turn stop row."),
        createAgentChunk("This turn intentionally has no terminal prompt response."),
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
      startedAt: createDebugTimestamp(14),
      messages: [
        createPromptMessage(
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
        createAgentChunk("Completed turn content with a resource link prompt."),
        createPromptResult("qa-prompt-completed"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-stopped",
      sequence: 3,
      promptRequestId: "qa-prompt-stopped",
      startedAt: createDebugTimestamp(21),
      stopReason: "max_tokens",
      messages: [
        createPromptMessage("qa-prompt-stopped", "Show a stopped turn."),
        createAgentChunk("This generated turn reaches a stop reason before normal completion."),
        createPromptResult("qa-prompt-stopped", "max_tokens"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-failed",
      sequence: 4,
      promptRequestId: "qa-prompt-failed",
      startedAt: createDebugTimestamp(28),
      completionKind: "error",
      stopReason: null,
      messages: [
        createPromptMessage("qa-prompt-failed", "Show a failed turn."),
        createAgentChunk("The next frame is a prompt error so the failure row has a reason."),
        createPromptError("qa-prompt-failed", "Generated ACP failure for manual QA."),
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
      startedAt: createDebugTimestamp(0),
      messages: [
        createPromptMessage("qa-prompt-oldest", "Oldest generated prompt."),
        createAgentChunk("This row appears only after loading the second older history page."),
        createPromptResult("qa-prompt-oldest"),
      ],
    }),
    createTurn({
      turnId: "qa-turn-cancelled",
      sequence: 1,
      promptRequestId: "qa-prompt-cancelled",
      startedAt: createDebugTimestamp(7),
      stopReason: "cancelled",
      messages: [
        createPromptMessage("qa-prompt-cancelled", "Show a cancelled turn."),
        createAgentChunk("The prompt is cancelled to exercise cancellation wording."),
        createPromptResult("qa-prompt-cancelled", "cancelled"),
      ],
    }),
  ]
}

function createInitialHistory() {
  return createHistoryPage({
    turns: buildInitialTurns(),
    nextCursor: FIRST_OLDER_CURSOR,
    hasMore: true,
  })
}

function findOlderHistoryPage(cursor: string | null) {
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

function createChat() {
  return new SessionChat({
    history: createInitialHistory(),
    session: createDebugSession(),
  })
}

function createChatWithAllHistory() {
  const chat = createChat()

  while (chat.nextCursor) {
    const page = findOlderHistoryPage(chat.nextCursor)

    if (!page) {
      break
    }

    chat.prependOlderHistory(page)
  }

  return chat
}

function createStatusCases() {
  return [
    {
      label: "Ready",
      description: "No running turn; composer should be usable.",
      session: createDebugSession({
        status: "active",
        activeDaemonSession: true,
        blockedReason: null,
      }),
      history: createHistoryPage({ turns: [], nextCursor: null, hasMore: false }),
    },
    {
      label: "Running",
      description: "Active daemon session with a running turn; stop should be available.",
      session: createDebugSession({
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
            startedAt: createDebugTimestamp(0),
            completedAt: null,
            completionKind: null,
            stopReason: null,
            messages: [
              createPromptMessage("qa-status-prompt-running", "Running status case."),
              createAgentChunk("Still working."),
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
      session: createDebugSession(),
      history: createInitialHistory(),
    },
    {
      label: "Reconnectable",
      description: "Live-capable session without an active daemon; reconnect should be available.",
      session: createDebugSession({ status: "active", blockedReason: null }),
      history: createHistoryPage({
        turns: buildInitialTurns().slice(0, 1),
        nextCursor: null,
        hasMore: false,
      }),
    },
    {
      label: "Completed",
      description: "Done session; only passive transcript actions should remain.",
      session: createDebugSession({
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
      session: createDebugSession({
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
      session: createDebugSession({
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
  ] satisfies DebugStatusCase[]
}

function createStatusSummary() {
  return createStatusCases().map((statusCase) => {
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

export function createSessionChatDebugController() {
  let chat = createChat()
  let olderHistoryError: string | null = null
  let version = 0
  const statusSummary = createStatusSummary()

  function refresh() {
    version += 1
  }

  return {
    get canLoadOlderHistory() {
      return chat.hasMore && chat.nextCursor !== null
    },
    get chat() {
      return chat
    },
    get olderHistoryError() {
      return olderHistoryError
    },
    get statusSummary() {
      return statusSummary
    },
    get version() {
      return version
    },
    loadAllHistory() {
      chat = createChatWithAllHistory()
      olderHistoryError = null
      refresh()
    },
    loadOlderHistory() {
      if (!(chat.hasMore && chat.nextCursor !== null)) {
        return
      }

      const page = findOlderHistoryPage(chat.nextCursor)

      if (!page) {
        olderHistoryError = `No generated page exists for ${chat.nextCursor}.`
        refresh()
        return
      }

      chat.prependOlderHistory(page)
      olderHistoryError = null
      refresh()
    },
    reset() {
      chat = createChat()
      olderHistoryError = null
      refresh()
    },
    resolvePermission(input: { optionId: string; requestId: string | number }) {
      chat.applyMessage(createSelectedPermissionResponse(input), {
        receivedAt: "2026-04-14T15:12:00.000Z",
      })
      refresh()
    },
    showPagingError() {
      olderHistoryError = "Generated history request failed."
      refresh()
    },
  }
}
