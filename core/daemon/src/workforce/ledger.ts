import type {
  WorkforceLedgerEvent,
  WorkforceProjection,
  WorkforceRequestRecord,
  WorkforceTruncateEvent,
} from "@goddard-ai/schema/workforce"
import { appendFile } from "node:fs/promises"
import { buildWorkforcePaths } from "./paths.ts"

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && Array.isArray(value) === false
}

function createRequestRecord(
  event: Extract<WorkforceLedgerEvent, { type: "request" }>,
): WorkforceRequestRecord {
  return {
    id: event.requestId,
    toAgentId: event.toAgentId,
    fromAgentId: event.fromAgentId,
    intent: event.intent,
    input: event.input,
    updates: [],
    status: "queued",
    createdAt: event.at,
    updatedAt: event.at,
    attemptCount: 0,
    activeSessionId: null,
    response: null,
    suspendedReason: null,
    errorMessage: null,
    cancelledReason: null,
  }
}

export function summarizeWorkforceProjection(requests: Record<string, WorkforceRequestRecord>) {
  const values = Object.values(requests)

  return {
    activeRequestCount: values.filter((request) => request.status === "active").length,
    queuedRequestCount: values.filter((request) => request.status === "queued").length,
    suspendedRequestCount: values.filter((request) => request.status === "suspended").length,
    failedRequestCount: values.filter((request) => request.status === "errored").length,
  }
}

export function buildWorkforceQueues(
  requests: Record<string, WorkforceRequestRecord>,
): Record<string, string[]> {
  const queues = new Map<string, string[]>()

  for (const request of Object.values(requests).sort((left, right) =>
    left.createdAt.localeCompare(right.createdAt),
  )) {
    if (request.status !== "queued") {
      continue
    }

    const queue = queues.get(request.toAgentId) ?? []
    queue.push(request.id)
    queues.set(request.toAgentId, queue)
  }

  return Object.fromEntries(queues)
}

function applyTruncate(
  requests: Record<string, WorkforceRequestRecord>,
  event: WorkforceTruncateEvent,
): void {
  for (const request of Object.values(requests)) {
    if (event.agentId !== null && request.toAgentId !== event.agentId) {
      continue
    }

    if (request.status !== "queued" && request.status !== "suspended") {
      continue
    }

    request.status = "cancelled"
    request.cancelledReason = event.reason ?? "Truncated by workforce policy."
    request.updatedAt = event.at
  }
}

export function applyWorkforceEvent(
  requests: Record<string, WorkforceRequestRecord>,
  event: WorkforceLedgerEvent,
): Record<string, WorkforceRequestRecord> {
  if (event.type === "request") {
    requests[event.requestId] = createRequestRecord(event)
    return requests
  }

  if (event.type === "truncate") {
    applyTruncate(requests, event)
    return requests
  }

  const request = requests[event.requestId]
  if (!request) {
    throw new Error(`Unknown workforce request: ${event.requestId}`)
  }

  switch (event.type) {
    case "handle":
      request.status = "active"
      request.updatedAt = event.at
      request.attemptCount = event.attempt
      request.activeSessionId = event.sessionId
      break
    case "response":
      request.status = "completed"
      request.updatedAt = event.at
      request.response = event.output
      request.activeSessionId = null
      request.suspendedReason = null
      break
    case "suspend":
      request.status = "suspended"
      request.updatedAt = event.at
      request.suspendedReason = event.reason
      request.activeSessionId = null
      break
    case "cancel":
      request.status = "cancelled"
      request.updatedAt = event.at
      request.cancelledReason = event.reason
      request.activeSessionId = null
      break
    case "update":
      request.updatedAt = event.at
      request.updates.push(event.input)
      request.activeSessionId = null
      request.suspendedReason = null
      if (request.status === "suspended" || request.status === "active") {
        request.status = "queued"
      }
      break
    case "error":
      request.status = "errored"
      request.updatedAt = event.at
      request.errorMessage = event.message
      request.activeSessionId = null
      break
  }

  return requests
}

export async function readWorkforceLedger(rootDir: string): Promise<WorkforceLedgerEvent[]> {
  const paths = buildWorkforcePaths(rootDir)
  const content = await Bun.file(paths.ledgerPath)
    .text()
    .catch(() => "")

  return content
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line, index) => {
      const parsed = JSON.parse(line) as unknown
      if (isRecord(parsed) === false || typeof parsed.type !== "string") {
        throw new Error(`Invalid workforce ledger event at line ${index + 1}`)
      }
      return parsed as unknown as WorkforceLedgerEvent
    })
}

export async function appendWorkforceLedgerEvent(
  rootDir: string,
  event: WorkforceLedgerEvent,
): Promise<void> {
  const paths = buildWorkforcePaths(rootDir)
  await appendFile(paths.ledgerPath, `${JSON.stringify(event)}\n`, "utf-8")
}

export async function replayWorkforceProjection(rootDir: string): Promise<WorkforceProjection> {
  const requests: Record<string, WorkforceRequestRecord> = {}

  for (const event of await readWorkforceLedger(rootDir)) {
    applyWorkforceEvent(requests, event)
  }

  for (const request of Object.values(requests)) {
    if (request.status === "active") {
      request.status = "queued"
      request.activeSessionId = null
    }
  }

  return {
    requests,
    queues: buildWorkforceQueues(requests),
    summary: summarizeWorkforceProjection(requests),
  }
}
