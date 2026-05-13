/** Turn-history helpers shared by daemon session lifecycle management. */
import { randomUUID } from "node:crypto"
import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type {
  DaemonSession,
  InboxHeadline,
  InboxScope,
  SessionHistoryTurn,
} from "@goddard-ai/schema/daemon"

/** Stable request id used to identify one persisted prompt turn. */
export type SessionTurnPromptRequestId = string | number

/** In-memory active turn buffer mirrored into the durable turn-draft record. */
export type ActiveTurnBuffer<TDraftId extends string = string> = {
  turnId: string
  sequence: number
  promptRequestId: SessionTurnPromptRequestId
  startedAt: string
  messages: acp.AnyMessage[]
  inboxScope?: InboxScope | null
  inboxHeadline?: InboxHeadline | null
  flushTimer: ReturnType<typeof setTimeout> | null
  draftId: TDraftId | null
  touchedAttentionEntity?: boolean
}

/** Durable draft payload written into `sessionTurnDrafts`. */
export type SessionTurnDraftInput = {
  sessionId: DaemonSessionId
  turnId: string
  sequence: number
  promptRequestId: SessionTurnPromptRequestId
  startedAt: string
  updatedAt: string
  messages: acp.AnyMessage[]
}

/** Durable completed-turn payload written into `sessionTurns`. */
export type CompletedSessionTurnInput = {
  sessionId: DaemonSessionId
  turnId: string
  sequence: number
  promptRequestId: SessionTurnPromptRequestId
  startedAt: string
  completedAt: string | null
  completionKind: "result" | "error" | null
  stopReason: DaemonSession["stopReason"]
  inboxScope?: InboxScope | null
  inboxHeadline?: InboxHeadline | null
  messages: acp.AnyMessage[]
}

/** Stored turn shape shared by completed turns and in-progress turn projections. */
type PersistableSessionTurn = Omit<
  SessionHistoryTurn,
  "completedAt" | "completionKind" | "stopReason"
> & {
  completedAt: string | null
  completionKind: "result" | "error" | null
  stopReason: DaemonSession["stopReason"]
}

/** Minimal stored draft shape needed to project a history turn. */
type SessionTurnDraftRecord = {
  turnId: string
  sequence: number
  promptRequestId: SessionTurnPromptRequestId
  startedAt: string
  messages: acp.AnyMessage[]
  inboxScope?: InboxScope | null
  inboxHeadline?: InboxHeadline | null
}

/** Launch-time initial prompt details needed to persist one completed turn. */
type InitializedHistoryTurnSeed = {
  initialPromptRequestId: SessionTurnPromptRequestId | null
  initialPromptStartedAt: string | null
  initialPromptCompletedAt: string | null
  stopReason: SessionHistoryTurn["stopReason"]
  history: acp.AnyMessage[]
}

/** Parsed streamed text chunk shape that can be merged in stored turn history. */
type AgentMessageChunkParts = {
  params: Record<string, unknown>
  update: Record<string, unknown>
  content: Record<string, unknown> & {
    type: "text"
    text: string
  }
}

/** Returns true when one unknown value is a plain object record. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

/** Extracts one mergeable streamed agent text chunk from ACP session history. */
function getAgentMessageChunkParts(message: acp.AnyMessage) {
  if (
    !isRecord(message) ||
    "method" in message === false ||
    message.method !== acp.CLIENT_METHODS.session_update
  ) {
    return null
  }

  const params = "params" in message && isRecord(message.params) ? message.params : null
  if (!params) {
    return null
  }

  const update = isRecord(params.update) ? params.update : null
  if (!update || update.sessionUpdate !== "agent_message_chunk") {
    return null
  }

  const content = isRecord(update.content) ? update.content : null
  if (!content || content.type !== "text" || typeof content.text !== "string") {
    return null
  }

  return {
    params,
    update,
    content: {
      ...content,
      type: "text",
      text: content.text,
    },
  } satisfies AgentMessageChunkParts
}

/** Appends one ACP history entry while folding adjacent streamed agent text chunks together. */
export function appendSessionHistoryMessage(history: acp.AnyMessage[], message: acp.AnyMessage) {
  const previousChunk = history.length > 0 ? getAgentMessageChunkParts(history.at(-1)!) : null
  const nextChunk = getAgentMessageChunkParts(message)

  if (previousChunk && nextChunk) {
    history[history.length - 1] = {
      ...message,
      params: {
        ...nextChunk.params,
        update: {
          ...nextChunk.update,
          content: {
            ...nextChunk.content,
            text: `${previousChunk.content.text}${nextChunk.content.text}`,
          },
        },
      },
    }
    return
  }

  history.push(message)
}

/** Rebuilds one history stream using the daemon's chunk-coalescing persistence rules. */
export function coalesceSessionHistoryMessages(messages: readonly acp.AnyMessage[]) {
  const history: acp.AnyMessage[] = []

  for (const message of messages) {
    appendSessionHistoryMessage(history, message)
  }

  return history
}

/** Extracts slash-command availability when one ACP update carries a full replacement list. */
export function getAvailableCommandsFromMessage(message: acp.AnyMessage) {
  if (
    !isRecord(message) ||
    "method" in message === false ||
    message.method !== acp.CLIENT_METHODS.session_update ||
    !isRecord(message.params)
  ) {
    return null
  }

  const update = isRecord(message.params.update) ? message.params.update : null
  if (
    !update ||
    update.sessionUpdate !== "available_commands_update" ||
    Array.isArray(update.availableCommands) === false
  ) {
    return null
  }

  return update.availableCommands.filter((command): command is acp.AvailableCommand => {
    return (
      isRecord(command) &&
      typeof command.name === "string" &&
      typeof command.description === "string"
    )
  })
}

/** Extracts the latest ACP slash-command update recorded in one message sequence. */
export function getLatestAvailableCommands(messages: readonly acp.AnyMessage[]) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const availableCommands = getAvailableCommandsFromMessage(messages[index])

    if (availableCommands !== null) {
      return availableCommands
    }
  }

  return null
}

/** Converts one stored turn shape into the API-facing session history turn value. */
function toSessionHistoryTurn(record: PersistableSessionTurn) {
  return {
    turnId: record.turnId,
    sequence: record.sequence,
    promptRequestId: record.promptRequestId,
    startedAt: record.startedAt,
    completedAt: record.completedAt,
    completionKind: record.completionKind,
    stopReason: record.stopReason,
    inboxScope: record.inboxScope ?? null,
    inboxHeadline: record.inboxHeadline ?? null,
    messages: [...record.messages],
  } satisfies SessionHistoryTurn
}

/** Converts one completed turn row into the API-facing history turn shape. */
export function toSessionHistoryTurnFromRecord(record: SessionHistoryTurn) {
  return toSessionHistoryTurn(record)
}

/** Converts one draft row into the API-facing history turn shape. */
export function toSessionHistoryTurnFromDraft(record: SessionTurnDraftRecord) {
  return toSessionHistoryTurn({
    ...record,
    completedAt: null,
    completionKind: null,
    stopReason: null,
    inboxScope: record.inboxScope ?? null,
    inboxHeadline: record.inboxHeadline ?? null,
  })
}

/** Converts one active-turn buffer into the API-facing history turn shape. */
export function toSessionHistoryTurnFromActiveTurn<TDraftId extends string>(
  record: ActiveTurnBuffer<TDraftId>,
) {
  return toSessionHistoryTurn({
    turnId: record.turnId,
    sequence: record.sequence,
    promptRequestId: record.promptRequestId,
    startedAt: record.startedAt,
    completedAt: null,
    completionKind: null,
    stopReason: null,
    inboxScope: record.inboxScope ?? null,
    inboxHeadline: record.inboxHeadline ?? null,
    messages: [...record.messages],
  })
}

/** Builds the durable payload stored in the mutable turn-draft kind. */
export function toTurnDraftInput(
  sessionId: DaemonSessionId,
  activeTurn: ActiveTurnBuffer,
  updatedAt = new Date().toISOString(),
) {
  return {
    sessionId,
    turnId: activeTurn.turnId,
    sequence: activeTurn.sequence,
    promptRequestId: activeTurn.promptRequestId,
    startedAt: activeTurn.startedAt,
    updatedAt,
    messages: [...activeTurn.messages],
  } satisfies SessionTurnDraftInput
}

/** Builds the durable payload stored in the completed-turn kind. */
export function toCompletedTurnInput(sessionId: DaemonSessionId, turn: SessionHistoryTurn) {
  return {
    sessionId,
    turnId: turn.turnId,
    sequence: turn.sequence,
    promptRequestId: turn.promptRequestId,
    startedAt: turn.startedAt,
    completedAt: turn.completedAt,
    completionKind: turn.completionKind,
    stopReason: turn.stopReason,
    inboxScope: turn.inboxScope ?? null,
    inboxHeadline: turn.inboxHeadline ?? null,
    messages: [...turn.messages],
  } satisfies CompletedSessionTurnInput
}

/** Returns true when one ACP message terminates the active prompt turn. */
export function isTurnTerminalMessage<TDraftId extends string>(
  activeTurn: ActiveTurnBuffer<TDraftId>,
  message: acp.AnyMessage,
) {
  return (
    "id" in message &&
    message.id != null &&
    message.id === activeTurn.promptRequestId &&
    ("result" in message || "error" in message)
  )
}

/** Returns true when one turn-scoped ACP message should force an immediate draft flush. */
export function shouldFlushTurnDraftImmediately<TDraftId extends string>(
  activeTurn: ActiveTurnBuffer<TDraftId>,
  message: acp.AnyMessage,
) {
  if (
    isRecord(message) &&
    "method" in message &&
    message.method === acp.CLIENT_METHODS.session_update &&
    isRecord(message.params) &&
    isRecord(message.params.update)
  ) {
    return (
      message.params.update.sessionUpdate === "tool_call" ||
      message.params.update.sessionUpdate === "tool_call_update"
    )
  }

  if (
    isRecord(message) &&
    "method" in message &&
    message.method === acp.AGENT_METHODS.session_cancel
  ) {
    return true
  }

  return isTurnTerminalMessage(activeTurn, message)
}

/** Builds the persisted completed turn emitted by launch-time `initialPrompt`, when present. */
export function createInitializedHistoryTurn(params: {
  initialized: InitializedHistoryTurnSeed
  sequence: number
}) {
  if (
    params.initialized.initialPromptRequestId === null ||
    params.initialized.initialPromptStartedAt === null ||
    params.initialized.initialPromptCompletedAt === null
  ) {
    return null
  }

  return {
    turnId: randomUUID(),
    sequence: params.sequence,
    promptRequestId: params.initialized.initialPromptRequestId,
    startedAt: params.initialized.initialPromptStartedAt,
    completedAt: params.initialized.initialPromptCompletedAt,
    completionKind: "result",
    stopReason: params.initialized.stopReason,
    inboxScope: null,
    inboxHeadline: null,
    messages: coalesceSessionHistoryMessages(params.initialized.history),
  } satisfies SessionHistoryTurn
}
