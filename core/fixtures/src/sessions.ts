import type {
  DaemonSession,
  GetSessionChangesResponse,
  GetSessionHistoryResponse,
  GetSessionResponse,
  GetSessionWorktreeResponse,
  ListSessionsResponse,
  SessionHistoryMessage,
  SessionHistoryTurn,
  SessionLaunchPreviewResponse,
} from "@goddard-ai/session/schema"

import { fixtureId, fixtureSessionId } from "./ids.ts"
import { fixtureNow, fixtureProjectPath, fixtureTimestamp } from "./time.ts"

type ModelConfigOption = SessionLaunchPreviewResponse["configOptions"][number]

export function createFixtureModelConfigOption(input: {
  currentValue: string
  models: Array<{ modelId: string; name: string; description?: string }>
}): ModelConfigOption {
  return {
    id: "model",
    type: "select",
    name: "Model",
    category: "model",
    currentValue: input.currentValue,
    options: input.models.map((model) => ({
      value: model.modelId,
      name: model.name,
      description: model.description,
    })),
  }
}

export function createFixtureSession(overrides: Partial<DaemonSession> = {}): DaemonSession {
  const id = overrides.id ?? fixtureSessionId("session_1")
  const status = overrides.status ?? "active"
  const activeDaemonSession =
    overrides.activeDaemonSession ?? (status === "active" || status === "blocked")

  return {
    id,
    acpSessionId: overrides.acpSessionId ?? `${id}_acp`,
    activeDaemonSession,
    agent: "codex",
    agentName: "Codex",
    availableCommands: [],
    blockedReason: null,
    completedHidden: false,
    configOptions: [],
    connectionMode: overrides.connectionMode ?? (status === "done" ? "history" : "live"),
    contextUsage: null,
    createdAt: fixtureNow - 86_400_000,
    cwd: fixtureProjectPath,
    errorMessage: null,
    inboxScope: null,
    initiative: null,
    lastAgentMessage: null,
    lastSessionActivityAt: fixtureNow,
    mcpServers: [],
    metadata: null,
    permissions: null,
    prNumber: null,
    repository: "goddard-ai",
    status,
    stopReason: null,
    supportsLoadSession: true,
    title: "Fixture session",
    titleState: "generated",
    token: null,
    ...overrides,
  }
}

export function createListSessionsResponse(
  sessions: DaemonSession[] = [createFixtureSession()],
  overrides: Partial<Omit<ListSessionsResponse, "sessions">> = {},
): ListSessionsResponse {
  return {
    hasMore: false,
    nextCursor: null,
    sessions,
    ...overrides,
  }
}

export function createGetSessionResponse(
  session: DaemonSession = createFixtureSession(),
): GetSessionResponse {
  return { session }
}

export function createSessionPromptMessage(input: {
  session: DaemonSession
  requestId?: string
  sequence?: number
  text?: string
}): SessionHistoryMessage {
  const sequence = input.sequence ?? 0
  return {
    sequence,
    sequenceStart: sequence,
    message: {
      jsonrpc: "2.0",
      id: input.requestId ?? fixtureId("req", input.session.id),
      method: "session/prompt",
      params: {
        prompt: [{ text: input.text ?? "Review the current fixture state.", type: "text" }],
        sessionId: input.session.acpSessionId,
      },
    },
  } satisfies SessionHistoryMessage
}

export function createSessionAgentChunkMessage(input: {
  session: DaemonSession
  sequence?: number
  text?: string
}): SessionHistoryMessage {
  const sequence = input.sequence ?? 0
  return {
    sequence,
    sequenceStart: sequence,
    message: {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: input.session.acpSessionId,
        update: {
          content: { text: input.text ?? "Fixture agent update.", type: "text" },
          sessionUpdate: "agent_message_chunk",
        },
      },
    },
  } satisfies SessionHistoryMessage
}

export function createSessionPermissionRequestMessage(input: {
  session: DaemonSession
  requestId?: string
  sequence?: number
  toolCallId?: string
  title?: string
}): SessionHistoryMessage {
  const sequence = input.sequence ?? 0
  return {
    sequence,
    sequenceStart: sequence,
    message: {
      jsonrpc: "2.0",
      id: input.requestId ?? fixtureId("req", "permission"),
      method: "session/request_permission",
      params: {
        options: [
          { kind: "allow_once", name: "Allow once", optionId: "allow_once" },
          { kind: "reject_once", name: "Reject", optionId: "reject" },
        ],
        sessionId: input.session.acpSessionId,
        toolCall: {
          kind: "edit",
          locations: [{ line: 1, path: "app/src/lib/query.ts" }],
          status: "pending",
          title: input.title ?? "Edit fixture-backed state",
          toolCallId: input.toolCallId ?? fixtureId("tool", "permission"),
        },
      },
    },
  } satisfies SessionHistoryMessage
}

export function createSessionHistoryTurn(
  input: Partial<SessionHistoryTurn> & {
    messages?: SessionHistoryMessage[]
  } = {},
): SessionHistoryTurn {
  return {
    completedAt: null,
    completionKind: null,
    inboxHeadline: null,
    inboxScope: null,
    messages: [],
    promptRequestId: fixtureId("req", "prompt"),
    sequence: 1,
    startedAt: fixtureTimestamp,
    stopReason: null,
    turnId: fixtureId("turn", "session_1"),
    ...input,
  }
}

export function createSessionHistoryResponse(
  input: {
    session?: DaemonSession
    turns?: Array<Partial<SessionHistoryTurn> & { messages?: SessionHistoryMessage[] }>
    overrides?: Partial<Omit<GetSessionHistoryResponse, "id" | "acpSessionId" | "turns">>
  } = {},
): GetSessionHistoryResponse {
  const session = input.session ?? createFixtureSession()

  return {
    id: session.id,
    acpSessionId: session.acpSessionId,
    connection: {
      activeDaemonSession: session.activeDaemonSession,
      mode: session.connectionMode,
      reconnectable: session.connectionMode === "live",
    },
    hasMore: false,
    nextCursor: null,
    turns: input.turns?.map((turn) => createSessionHistoryTurn(turn)) ?? [],
    ...input.overrides,
  }
}

export function createSessionWorktreeResponse(
  input: {
    session?: DaemonSession
    worktree?: GetSessionWorktreeResponse["worktree"]
  } = {},
): GetSessionWorktreeResponse {
  const session = input.session ?? createFixtureSession()

  return {
    id: session.id,
    acpSessionId: session.acpSessionId,
    worktree: input.worktree ?? {
      branchName: "codex/fixtures",
      effectiveCwd: fixtureProjectPath,
      poweredBy: "goddard",
      repoRoot: fixtureProjectPath,
      requestedCwd: fixtureProjectPath,
      worktreeDir: "/Users/alec/.codex/worktrees/fixtures/goddard-ai",
    },
  }
}

export function createSessionChangesResponse(
  input: {
    session?: DaemonSession
    diff?: string
    workspaceRoot?: string | null
  } = {},
): GetSessionChangesResponse {
  const session = input.session ?? createFixtureSession()
  const diff = input.diff ?? ""

  return {
    id: session.id,
    acpSessionId: session.acpSessionId,
    diff,
    hasChanges: diff.length > 0,
    workspaceRoot: input.workspaceRoot ?? fixtureProjectPath,
  }
}
