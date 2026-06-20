import { randomUUID } from "node:crypto"
import type { DaemonLogService } from "@goddard-ai/daemon-plugin"
import { IpcClientError } from "@goddard-ai/ipc"
import { isAcpRequest } from "acp-client"
import * as acp from "acp-client/protocol"
import { getErrorMessage } from "radashi"

import type { SessionDb } from "../daemon.ts"
import {
  SessionErrorCodes,
  type AbortedSessionPrompt,
  type CancelSessionResponse,
  type DaemonSession,
  type DaemonSessionStatus,
  type DaemonSessionTurnDraft,
  type PopQueuedSessionPromptResponse,
  type SessionLifecycleField,
  type SessionMessageEvent,
  type SessionTurnMessage,
  type SteerSessionResponse,
} from "../schema.ts"
import type { createActiveTurnStore } from "./active-turns.ts"
import type { createIdleShutdownController } from "./idle-shutdown.ts"
import type { SessionEventEmitter } from "./manager.ts"
import type { ActiveSession, QueuedPromptEntry, SessionMemory } from "./session-memory.ts"
import { resolveLatestStoredTurnSequence } from "./session-records.ts"
import type { createSessionTitleRuntime } from "./session-titles-runtime.ts"
import {
  appendSessionHistoryMessage,
  isContextUsageUpdateMessage,
  type ActiveTurnBuffer,
} from "./turn-history.ts"

type SessionId = DaemonSession["id"]
type SessionTurnDraftDoc = DaemonSessionTurnDraft

const QUEUED_PROMPT_ABORTED_ERROR_CODE = -32800
const QUEUED_PROMPT_ABORTED_ERROR_MESSAGE =
  "Queued prompt aborted before dispatch by session cancellation."

/** Captures prompt requests so their responses can drive status transitions. */
type PromptRequestMessage = acp.AnyMessage & {
  params: acp.PromptRequest
}

/** Narrows one agent notification to a structured session update payload. */
type SessionUpdateMessage = acp.AnyMessage & {
  params: acp.SessionNotification
}

/** Ensures the daemon's system prompt is prepended to the first user prompt sent to an agent. */
export function injectSystemPrompt(
  request: acp.PromptRequest,
  systemPrompt: string,
): acp.PromptRequest {
  if (systemPrompt.length === 0) {
    return request
  }

  return {
    ...request,
    prompt: [
      {
        type: "text",
        text: `<system-prompt name="goddard">${systemPrompt}</system-prompt>`,
      },
      ...request.prompt,
    ],
  }
}

/** Maps client-originated ACP messages to any immediate session status changes they imply. */
function sessionStatusFromClientMessage(
  message: acp.AnyMessage,
  status: DaemonSessionStatus,
): DaemonSessionStatus | null {
  if (status !== "active") {
    return null
  }

  if (isAcpRequest(message, acp.AGENT_METHODS.session_cancel)) {
    return "cancelled"
  }

  return null
}

/** Normalizes one queued prompt back into the client-facing aborted-queue payload. */
function toAbortedQueuedPrompt(entry: {
  requestId: string | number
  prompt: acp.ContentBlock[]
}): AbortedSessionPrompt {
  return {
    requestId: entry.requestId,
    prompt: [...entry.prompt],
  }
}

function normalizePrompt(prompt: string | acp.ContentBlock[]): acp.ContentBlock[] {
  return typeof prompt === "string" ? [{ type: "text", text: prompt }] : [...prompt]
}

/** Owns live prompt turn queueing, cancellation, steering, permission routing, and ACP publication. */
export function createPromptTurnFeature({
  db,
  memory,
  log,
  events,
  emitMessage,
  activeTurns,
  idleShutdown,
  sessionTitles,
  emitDiagnostic,
  publishSessionUpdated,
  updateSessionActivity,
}: {
  db: SessionDb
  memory: SessionMemory
  log: DaemonLogService
  events: SessionEventEmitter
  emitMessage: (id: SessionId, message: SessionMessageEvent) => void
  activeTurns: ReturnType<typeof createActiveTurnStore>
  idleShutdown: ReturnType<typeof createIdleShutdownController>
  sessionTitles: ReturnType<typeof createSessionTitleRuntime>
  emitDiagnostic: (
    sessionId: SessionId,
    type: string,
    detail?: Record<string, unknown>,
    diagnosticLogger?: ReturnType<DaemonLogService["createLogger"]>,
  ) => void
  publishSessionUpdated: (id: SessionId, changed: readonly SessionLifecycleField[]) => void
  updateSessionActivity: (
    id: SessionId,
    update: Partial<DaemonSession>,
    detail?: Record<string, unknown>,
    diagnosticLogger?: ReturnType<DaemonLogService["createLogger"]>,
  ) => void
}) {
  const activeSessions = memory.activeSessions
  const acpDebug = log.createDebug("session.acp")
  const queueDebug = log.createDebug("session.queue")

  function publishSessionMessage(
    active: ActiveSession,
    message: acp.AnyMessage,
    options: {
      persistTurnMessage?: boolean
      messageEvent?: SessionMessageEvent
    } = {},
  ) {
    const turnMessage =
      options.persistTurnMessage !== false
        ? activeTurns.appendTurnScopedMessage(active, message)
        : null
    const messageEvent = options.messageEvent ?? turnMessage
    if (messageEvent) {
      emitMessage(active.id, messageEvent)
      return
    }

    if (isContextUsageUpdateMessage(message)) {
      emitMessage(active.id, message)
      return
    }

    throw new Error("Session stream message is missing turn sequence metadata.")
  }

  function publishClientMessage(
    active: ActiveSession,
    message: acp.AnyMessage,
    options: {
      updateStatus?: boolean
      persistTurnMessage?: boolean
      onBeforePublish?: (message: acp.AnyMessage) => SessionMessageEvent | void
    } = {},
  ): void {
    if (options.updateStatus !== false) {
      const nextStatus = sessionStatusFromClientMessage(message, active.status)
      if (nextStatus) {
        updateSessionActivity(
          active.id,
          { status: nextStatus },
          {
            reason: "client_message",
            method: "method" in message ? message.method : undefined,
            messageId: "id" in message ? message.id : undefined,
          },
        )
      }
    }

    acpDebug("session.acp.message_write", {
      sessionId: active.id,
      acpSessionId: active.acpSessionId,
      hasId: "id" in message && message.id != null,
      method: "method" in message ? message.method : undefined,
      message: log.createPayloadPreview(message, { maxStringLength: 160 }),
    })
    emitDiagnostic(
      active.id,
      "session_message_sent",
      {
        hasId: "id" in message && message.id != null,
        method: "method" in message ? message.method : undefined,
      },
      active.logger,
    )
    const messageEvent = options.onBeforePublish?.(message) ?? undefined
    publishSessionMessage(active, message, {
      persistTurnMessage: options.persistTurnMessage,
      messageEvent,
    })
  }

  async function handleSessionUpdate(
    active: ActiveSession,
    params: acp.SessionNotification,
  ): Promise<void> {
    const message = {
      jsonrpc: "2.0",
      method: acp.CLIENT_METHODS.session_update,
      params,
    } satisfies acp.AnyMessage

    acpDebug("session.acp.message_read", {
      sessionId: active.id,
      acpSessionId: active.acpSessionId,
      hasId: false,
      method: acp.CLIENT_METHODS.session_update,
      updateType: params.update.sessionUpdate,
      message: log.createPayloadPreview(message, { maxStringLength: 160 }),
    })
    publishSessionMessage(active, message, {
      messageEvent: active.activeTurn
        ? undefined
        : createDetachedSessionMessageEvent(active, message),
    })
    await handleSteerBoundary(active, message)
  }

  async function handlePermissionRequest(
    active: ActiveSession,
    params: acp.RequestPermissionRequest,
  ): Promise<acp.RequestPermissionResponse> {
    const requestId =
      active.blockingPromptRequestId === null
        ? `permission-${randomUUID()}`
        : `permission-${String(active.blockingPromptRequestId)}`

    return await new Promise<acp.RequestPermissionResponse>((resolve) => {
      active.lastPermissionRequest = {
        id: requestId,
        params,
        resolve,
      }
      publishSessionUpdated(active.id, ["permission"])
      idleShutdown.refreshIdleShutdownState(active.id, "permission_request_started")
      const message = {
        jsonrpc: "2.0",
        id: requestId,
        method: acp.CLIENT_METHODS.session_request_permission,
        params,
      } satisfies acp.AnyMessage

      acpDebug("session.acp.message_read", {
        sessionId: active.id,
        acpSessionId: active.acpSessionId,
        hasId: true,
        method: acp.CLIENT_METHODS.session_request_permission,
        message: log.createPayloadPreview(message, { maxStringLength: 160 }),
      })
      publishSessionMessage(active, message)
    })
  }

  function updateSessionFromPromptResponse(
    active: ActiveSession,
    requestId: string | number,
    response: acp.PromptResponse,
  ) {
    const stopReason = response.stopReason ?? null
    const nextStatus = stopReason === "end_turn" ? "done" : null

    if (nextStatus || stopReason) {
      updateSessionActivity(
        active.id,
        {
          ...(nextStatus && { status: nextStatus }),
          ...(stopReason && { stopReason }),
        },
        {
          reason: "agent_message",
          requestMethod: acp.AGENT_METHODS.session_prompt,
          responseId: requestId,
          stopReason: stopReason ?? undefined,
        },
      )
    }
  }

  function createDetachedSessionMessageEvent(
    active: ActiveSession,
    message: acp.AnyMessage,
    sequence = active.nextTurnSequence,
  ): SessionTurnMessage {
    // Steered prompts can receive late updates after their turn buffer has moved to the replacement.
    return {
      sequence,
      sequenceStart: sequence,
      message,
    }
  }

  function createDetachedPromptResponseEvent(
    active: ActiveSession,
    entry: QueuedPromptEntry,
    message: acp.AnyMessage,
  ): SessionTurnMessage | undefined {
    if (active.activeTurn?.promptRequestId === entry.requestId) {
      return undefined
    }

    return createDetachedSessionMessageEvent(active, message, entry.turnSequence)
  }

  async function completePrompt(
    active: ActiveSession,
    entry: QueuedPromptEntry,
    message: acp.AnyMessage & { params: acp.PromptRequest },
  ) {
    try {
      const response = await active.session.prompt(message.params.prompt)
      const responseMessage = {
        jsonrpc: "2.0",
        id: entry.requestId,
        result: response,
      } satisfies acp.AnyMessage

      acpDebug("session.acp.message_read", {
        sessionId: active.id,
        acpSessionId: active.acpSessionId,
        hasId: true,
        responseId: entry.requestId,
        responseKind: "result",
        message: log.createPayloadPreview(responseMessage, { maxStringLength: 160 }),
      })
      const detachedEvent = createDetachedPromptResponseEvent(active, entry, responseMessage)
      if (!detachedEvent) {
        updateSessionFromPromptResponse(active, entry.requestId, response)
      }
      entry.resolve?.(response)
      queueDebug("session.queue.prompt_completed", {
        sessionId: active.id,
        requestId: entry.requestId,
        turnSequence: entry.turnSequence,
        detached: Boolean(detachedEvent),
        queueLength: active.promptQueue.length,
      })

      if (active.blockingPromptRequestId === entry.requestId) {
        active.blockingPromptRequestId = null
      }

      publishSessionMessage(active, responseMessage, { messageEvent: detachedEvent })
      if (!detachedEvent) {
        activeTurns.finalizeActiveTurn(active, responseMessage)
      }
      await handleSteerBoundary(active, responseMessage)
      await processPromptQueue(active)
    } catch (error) {
      const responseMessage = {
        jsonrpc: "2.0",
        id: entry.requestId,
        error: {
          code: -32603,
          message: getErrorMessage(error),
        },
      } satisfies acp.AnyMessage

      acpDebug("session.acp.message_read", {
        sessionId: active.id,
        acpSessionId: active.acpSessionId,
        hasId: true,
        responseId: entry.requestId,
        responseKind: "error",
        errorMessage: getErrorMessage(error),
        message: log.createPayloadPreview(responseMessage, { maxStringLength: 160 }),
      })
      entry.reject?.(error instanceof Error ? error : new Error(getErrorMessage(error)))
      queueDebug("session.queue.prompt_failed", {
        sessionId: active.id,
        requestId: entry.requestId,
        turnSequence: entry.turnSequence,
        errorMessage: getErrorMessage(error),
        queueLength: active.promptQueue.length,
      })
      if (active.blockingPromptRequestId === entry.requestId) {
        active.blockingPromptRequestId = null
      }
      const detachedEvent = createDetachedPromptResponseEvent(active, entry, responseMessage)
      publishSessionMessage(active, responseMessage, { messageEvent: detachedEvent })
      if (!detachedEvent) {
        activeTurns.finalizeActiveTurn(active, responseMessage)
      }
      await handleSteerBoundary(active, responseMessage)
      await processPromptQueue(active)
    }
  }

  async function processPromptQueue(active: ActiveSession): Promise<void> {
    if (active.blockingPromptRequestId !== null || active.pendingSteer?.waitingForBoundary) {
      queueDebug("session.queue.dispatch_blocked", {
        sessionId: active.id,
        blockingPromptRequestId: active.blockingPromptRequestId,
        pendingSteerRequestId: active.pendingSteer?.requestId,
        waitingForBoundary: active.pendingSteer?.waitingForBoundary,
        queueLength: active.promptQueue.length,
      })
      return
    }

    const nextPrompt = active.promptQueue.shift()
    if (!nextPrompt) {
      queueDebug("session.queue.dispatch_skipped", {
        sessionId: active.id,
        reason: "empty_queue",
      })
      return
    }
    queueDebug("session.queue.prompt_dequeued", {
      sessionId: active.id,
      requestId: nextPrompt.requestId,
      source: nextPrompt.source,
      remainingQueueLength: active.promptQueue.length,
    })
    publishSessionUpdated(active.id, ["queue"])

    const promptRequest = {
      sessionId: active.acpSessionId,
      prompt: [...nextPrompt.prompt],
    }
    const message = {
      jsonrpc: "2.0",
      id: nextPrompt.requestId,
      method: acp.AGENT_METHODS.session_prompt,
      params:
        active.isFirstPrompt === true
          ? injectSystemPrompt(promptRequest, active.systemPrompt)
          : promptRequest,
    } satisfies acp.AnyMessage & {
      id: string | number
      method: string
      params: acp.PromptRequest
    }
    active.isFirstPrompt = false
    const existingDraft =
      db.sessionTurnDrafts.first({
        where: { sessionId: active.id },
      }) ?? null
    if (existingDraft) {
      activeTurns.persistTurnDraftAsInterruptedTurn(active.id, existingDraft, active.logger)
      active.nextTurnSequence = resolveLatestStoredTurnSequence(db, active.id) + 1
    }

    const activeTurn: ActiveTurnBuffer<SessionTurnDraftDoc["id"]> = {
      turnId: randomUUID(),
      sequence: active.nextTurnSequence,
      promptRequestId: nextPrompt.requestId,
      startedAt: new Date().toISOString(),
      messages: [],
      inboxScope: null,
      inboxHeadline: null,
      flushTimer: null,
      draftId: null,
      touchedAttentionEntity: false,
    }
    active.activeTurn = activeTurn
    nextPrompt.turnSequence = activeTurn.sequence
    // Claim the blocking slot before the write so overlapping prompt dispatches stay serialized.
    active.blockingPromptRequestId = nextPrompt.requestId

    publishSessionUpdated(active.id, ["activeTurn"])
    idleShutdown.refreshIdleShutdownState(active.id, "turn_started")

    try {
      emitDiagnostic(
        active.id,
        "session_turn_started",
        {
          turnId: activeTurn.turnId,
          sequence: activeTurn.sequence,
          promptRequestId: activeTurn.promptRequestId,
        },
        active.logger,
      )
      queueDebug("session.queue.prompt_dispatching", {
        sessionId: active.id,
        requestId: nextPrompt.requestId,
        source: nextPrompt.source,
        turnId: activeTurn.turnId,
        sequence: activeTurn.sequence,
      })
      publishClientMessage(active, message, {
        persistTurnMessage: false,
        onBeforePublish: (resolvedMessage) => {
          const turnMessage = appendSessionHistoryMessage(activeTurn.messages, resolvedMessage)
          activeTurns.flushActiveTurnDraft(active, "start")
          return turnMessage ?? undefined
        },
      })
      void completePrompt(active, nextPrompt, message)
    } catch (error) {
      if (activeTurn.draftId) {
        db.sessionTurnDrafts.delete(activeTurn.draftId)
      }
      activeTurns.clearTurnDraftFlushTimer(active.activeTurn)
      active.activeTurn = null
      if (active.blockingPromptRequestId === nextPrompt.requestId) {
        active.blockingPromptRequestId = null
      }
      publishSessionUpdated(active.id, ["activeTurn"])
      idleShutdown.refreshIdleShutdownState(active.id, "turn_start_failed")
      nextPrompt.reject?.(error instanceof Error ? error : new Error(getErrorMessage(error)))
      throw error
    }
  }

  async function abortQueuedPrompts(
    active: ActiveSession,
    reason: string,
    options: {
      includePendingSteer?: boolean
    } = {},
  ): Promise<AbortedSessionPrompt[]> {
    const abortedQueue: AbortedSessionPrompt[] = []

    if (options.includePendingSteer && active.pendingSteer) {
      const pendingSteer = active.pendingSteer
      active.pendingSteer = null
      abortedQueue.push(
        toAbortedQueuedPrompt({
          requestId: pendingSteer.requestId,
          prompt: pendingSteer.prompt,
        }),
      )
      pendingSteer.reject(new IpcClientError(reason))
    }

    while (active.promptQueue.length > 0) {
      const queuedPrompt = active.promptQueue.shift()!
      abortedQueue.push(toAbortedQueuedPrompt(queuedPrompt))
      if (queuedPrompt.source === "client") {
        // Raw ACP callers need a terminal JSON-RPC response because this prompt never reached the agent.
        publishSessionMessage(active, {
          jsonrpc: "2.0",
          id: queuedPrompt.requestId,
          error: {
            code: QUEUED_PROMPT_ABORTED_ERROR_CODE,
            message: QUEUED_PROMPT_ABORTED_ERROR_MESSAGE,
          },
        })
        continue
      }

      queuedPrompt.reject?.(new IpcClientError(reason))
    }

    queueDebug("session.queue.prompts_aborted", {
      sessionId: active.id,
      reason,
      abortedQueueLength: abortedQueue.length,
      includedPendingSteer: Boolean(options.includePendingSteer),
    })
    publishSessionUpdated(active.id, ["queue"])
    idleShutdown.refreshIdleShutdownState(active.id, "queued_prompts_aborted")
    return abortedQueue
  }

  function abortPendingSteer(active: ActiveSession, reason: string): AbortedSessionPrompt[] {
    if (!active.pendingSteer) {
      return []
    }

    const pendingSteer = active.pendingSteer
    active.pendingSteer = null
    pendingSteer.reject(new IpcClientError(reason))
    queueDebug("session.queue.pending_steer_aborted", {
      sessionId: active.id,
      requestId: pendingSteer.requestId,
      cancelledRequestId: pendingSteer.cancelledRequestId,
      reason,
    })
    return [
      toAbortedQueuedPrompt({
        requestId: pendingSteer.requestId,
        prompt: pendingSteer.prompt,
      }),
    ]
  }

  async function sendInternalCancel(
    active: ActiveSession,
    options: {
      updateStatus: boolean
    },
  ): Promise<boolean> {
    if (active.blockingPromptRequestId === null) {
      queueDebug("session.queue.cancel_skipped", {
        sessionId: active.id,
        reason: "no_blocking_prompt",
      })
      return false
    }

    publishClientMessage(
      active,
      {
        jsonrpc: "2.0",
        method: acp.AGENT_METHODS.session_cancel,
        params: {
          sessionId: active.acpSessionId,
        },
      },
      { updateStatus: options.updateStatus },
    )
    await active.session.cancel()
    queueDebug("session.queue.cancel_sent", {
      sessionId: active.id,
      blockingPromptRequestId: active.blockingPromptRequestId,
      updateStatus: options.updateStatus,
    })

    return true
  }

  async function cancelSessionTurn(
    id: SessionId,
    options: {
      includePendingSteer?: boolean
      updateStatus: boolean
    } = { updateStatus: true },
  ): Promise<CancelSessionResponse> {
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError({
        code: SessionErrorCodes.NotActive,
        details: { sessionId: id },
        message: `Session ${id} is not active`,
      })
    }

    const abortedQueue = await abortQueuedPrompts(
      active,
      `Queued prompts were aborted for session ${id}.`,
      {
        includePendingSteer: options.includePendingSteer ?? true,
      },
    )
    const activeTurnCancelled = await sendInternalCancel(active, {
      updateStatus: options.updateStatus,
    })

    emitDiagnostic(id, "session_turn_cancelled", {
      activeTurnCancelled,
      abortedQueueLength: abortedQueue.length,
    })

    return {
      id,
      activeTurnCancelled,
      abortedQueue,
    }
  }

  async function handleSteerBoundary(
    active: ActiveSession,
    message: acp.AnyMessage,
  ): Promise<void> {
    const steer = active.pendingSteer
    if (!steer?.waitingForBoundary) {
      return
    }

    const reachedBoundary = isAcpRequest<SessionUpdateMessage>(
      message,
      acp.CLIENT_METHODS.session_update,
    )
      ? message.params.update.sessionUpdate === "tool_call" ||
        message.params.update.sessionUpdate === "tool_call_update"
      : "id" in message && message.id != null && message.id === steer.cancelledRequestId
    if (!reachedBoundary) {
      queueDebug("session.queue.steer_boundary_waiting", {
        sessionId: active.id,
        requestId: steer.requestId,
        cancelledRequestId: steer.cancelledRequestId,
        method: "method" in message ? message.method : undefined,
        hasId: "id" in message && message.id != null,
      })
      return
    }

    steer.waitingForBoundary = false
    if (active.blockingPromptRequestId === steer.cancelledRequestId) {
      active.blockingPromptRequestId = null
    }

    active.pendingSteer = null
    queueDebug("session.queue.steer_boundary_reached", {
      sessionId: active.id,
      requestId: steer.requestId,
      cancelledRequestId: steer.cancelledRequestId,
      abortedQueueLength: steer.abortedQueue.length,
    })
    try {
      const response = await promptSession(active.id, steer.prompt, { priority: "next" })
      steer.resolve({
        id: active.id,
        abortedQueue: steer.abortedQueue,
        response,
      })
    } catch (error) {
      idleShutdown.refreshIdleShutdownState(active.id, "steer_cleared")
      steer.reject(error instanceof Error ? error : new Error(getErrorMessage(error)))
    }
  }

  async function sendMessage(id: SessionId, message: acp.AnyMessage): Promise<void> {
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    if (isAcpRequest<PromptRequestMessage>(message, acp.AGENT_METHODS.session_prompt)) {
      if ("id" in message === false || message.id == null) {
        throw new IpcClientError("Queued prompt messages must include a JSON-RPC id")
      }

      sessionTitles.queueSessionTitlePreparation({
        id: active.id,
        prompt: message.params.prompt,
        diagnosticLogger: active.logger,
      })
      active.promptQueue.push({
        requestId: message.id,
        prompt: [...message.params.prompt],
        source: "client",
      })
      queueDebug("session.queue.prompt_enqueued", {
        sessionId: active.id,
        requestId: message.id,
        source: "client",
        queueLength: active.promptQueue.length,
      })
      publishSessionUpdated(active.id, ["queue"])
      updateSessionActivity(id, {
        completedHidden: false,
      })
      await events.emit("session.replied", {
        sessionId: id,
      })
      idleShutdown.refreshIdleShutdownState(active.id, "prompt_enqueued")
      emitDiagnostic(active.id, "session_prompt_enqueued", {
        requestId: message.id,
        queueLength: active.promptQueue.length,
      })
      await processPromptQueue(active)
      return
    }

    if (isAcpRequest(message, acp.AGENT_METHODS.session_cancel)) {
      await abortQueuedPrompts(active, `Queued prompts were aborted for session ${id}.`, {
        includePendingSteer: true,
      })
      publishClientMessage(active, message)
      await active.session.cancel()
      return
    }

    if (
      active.lastPermissionRequest &&
      "id" in message &&
      message.id === active.lastPermissionRequest.id &&
      "result" in message
    ) {
      const permissionRequest = active.lastPermissionRequest
      active.lastPermissionRequest = null
      publishSessionUpdated(active.id, ["permission"])
      idleShutdown.refreshIdleShutdownState(active.id, "permission_request_resolved")
      publishClientMessage(active, message)
      permissionRequest.resolve(message.result as acp.RequestPermissionResponse)
      return
    }

    throw new IpcClientError(`Unsupported ACP session message for active session ${id}`)
  }

  async function promptSession(
    id: SessionId,
    prompt: string | acp.ContentBlock[],
    options: {
      priority?: "next"
    } = {},
  ): Promise<acp.PromptResponse> {
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    sessionTitles.queueSessionTitlePreparation({
      id: active.id,
      prompt,
      diagnosticLogger: active.logger,
    })
    const requestId = randomUUID()
    const response = new Promise<acp.PromptResponse>((resolve, reject) => {
      const queuedPrompt = {
        requestId,
        prompt: normalizePrompt(prompt),
        source: "daemon",
        resolve,
        reject,
      } satisfies QueuedPromptEntry
      if (options.priority === "next") {
        active.promptQueue.unshift(queuedPrompt)
      } else {
        active.promptQueue.push(queuedPrompt)
      }
      queueDebug("session.queue.prompt_enqueued", {
        sessionId: active.id,
        requestId,
        source: "daemon",
        priority: options.priority ?? "normal",
        queueLength: active.promptQueue.length,
      })
    })

    updateSessionActivity(active.id, {})
    publishSessionUpdated(active.id, ["queue"])
    idleShutdown.refreshIdleShutdownState(active.id, "prompt_enqueued")
    emitDiagnostic(
      active.id,
      "session_prompt_enqueued",
      {
        requestId,
        queueLength: active.promptQueue.length,
      },
      active.logger,
    )
    await processPromptQueue(active)
    return await response
  }

  async function popQueuedPrompt(id: SessionId): Promise<PopQueuedSessionPromptResponse> {
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    let queuedPromptIndex = -1
    for (let index = active.promptQueue.length - 1; index >= 0; index -= 1) {
      if (active.promptQueue[index]?.source === "client") {
        queuedPromptIndex = index
        break
      }
    }
    if (queuedPromptIndex === -1) {
      return {
        id,
        prompt: null,
      }
    }

    const [queuedPrompt] = active.promptQueue.splice(queuedPromptIndex, 1)
    if (!queuedPrompt) {
      return {
        id,
        prompt: null,
      }
    }

    publishSessionUpdated(active.id, ["queue"])
    queueDebug("session.queue.prompt_popped", {
      sessionId: active.id,
      requestId: queuedPrompt.requestId,
      queueLength: active.promptQueue.length,
    })
    idleShutdown.refreshIdleShutdownState(active.id, "queued_prompt_popped")
    emitDiagnostic(active.id, "session_prompt_queue_popped", {
      requestId: queuedPrompt.requestId,
      queueLength: active.promptQueue.length,
    })

    return {
      id,
      prompt: toAbortedQueuedPrompt(queuedPrompt),
    }
  }

  async function steerSession(
    id: SessionId,
    prompt: string | acp.ContentBlock[],
  ): Promise<SteerSessionResponse> {
    const active = activeSessions.get(id)
    if (!active) {
      throw new IpcClientError(`Session ${id} is not active`)
    }

    const requestId = randomUUID()
    const abortedQueue = abortPendingSteer(
      active,
      `Pending steering was replaced for session ${id}.`,
    )

    if (active.blockingPromptRequestId === null) {
      queueDebug("session.queue.steer_immediate", {
        sessionId: active.id,
        requestId,
        abortedQueueLength: abortedQueue.length,
      })
      const response = await promptSession(id, prompt, { priority: "next" })
      return {
        id,
        abortedQueue,
        response,
      }
    }

    return await new Promise<SteerSessionResponse>((resolve, reject) => {
      // Keep tracking the cancelled prompt id so steering can wait for that turn's tool/final boundary.
      active.pendingSteer = {
        requestId,
        cancelledRequestId: active.blockingPromptRequestId!,
        prompt: normalizePrompt(prompt),
        abortedQueue,
        waitingForBoundary: true,
        resolve,
        reject,
      }
      queueDebug("session.queue.steer_started", {
        sessionId: active.id,
        requestId,
        cancelledRequestId: active.blockingPromptRequestId,
        abortedQueueLength: abortedQueue.length,
      })
      updateSessionActivity(active.id, {})
      idleShutdown.refreshIdleShutdownState(active.id, "steer_started")

      void sendInternalCancel(active, { updateStatus: false }).catch((error) => {
        if (active.pendingSteer?.requestId === requestId) {
          active.pendingSteer = null
        }
        reject(error instanceof Error ? error : new Error(getErrorMessage(error)))
      })
    })
  }

  return {
    cancelSessionTurn,
    handlePermissionRequest,
    handleSessionUpdate,
    promptSession,
    popQueuedPrompt,
    sendMessage,
    steerSession,
  }
}
