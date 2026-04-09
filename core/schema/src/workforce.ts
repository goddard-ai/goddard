import { AgentDistribution } from "./agent-distribution.ts"
import type { WorkforceRequestIntent } from "./workforce/requests.ts"

export type { WorkforceRequestIntent }

/** Supported workforce agent roles within one repository-owned runtime. */
export type WorkforceAgentRole = "root" | "domain"

/** Stable lifecycle states for one workforce request after replay. */
export type WorkforceRequestStatus =
  | "queued"
  | "active"
  | "suspended"
  | "completed"
  | "cancelled"
  | "errored"

/** One configured workforce agent and the repository paths it owns. */
export interface WorkforceAgentConfig {
  id: string
  name: string
  role: WorkforceAgentRole
  cwd: string
  owns: string[]
  agent?: string | AgentDistribution
}

/** Repository-local workforce configuration stored in `.goddard/workforce.json`. */
export interface WorkforceConfig {
  version: 1
  defaultAgent: string | AgentDistribution
  rootAgentId: string
  agents: WorkforceAgentConfig[]
}

/** Shared metadata carried by every append-only workforce ledger event. */
export interface WorkforceEventBase {
  id: string
  at: string
  type: "request" | "handle" | "response" | "suspend" | "cancel" | "update" | "error" | "truncate"
}

/** A new unit of work routed to one owning workforce agent. */
export interface WorkforceRequestEvent extends WorkforceEventBase {
  type: "request"
  requestId: string
  toAgentId: string
  fromAgentId: string | null
  intent: WorkforceRequestIntent
  input: string
}

/** A handle attempt recorded before the daemon launches a fresh agent session. */
export interface WorkforceHandleEvent extends WorkforceEventBase {
  type: "handle"
  requestId: string
  agentId: string
  attempt: number
  sessionId: string | null
}

/** A successful response that finishes the current request. */
export interface WorkforceResponseEvent extends WorkforceEventBase {
  type: "response"
  requestId: string
  agentId: string
  output: string
}

/** A suspended request that requires a later update before it can resume. */
export interface WorkforceSuspendEvent extends WorkforceEventBase {
  type: "suspend"
  requestId: string
  agentId: string
  reason: string
}

/** A request cancellation initiated by an operator or workflow policy. */
export interface WorkforceCancelEvent extends WorkforceEventBase {
  type: "cancel"
  requestId: string
  reason: string | null
}

/** A request update that appends context and resumes suspended work. */
export interface WorkforceUpdateEvent extends WorkforceEventBase {
  type: "update"
  requestId: string
  input: string
}

/** A fatal request failure recorded after retry budget exhaustion. */
export interface WorkforceErrorEvent extends WorkforceEventBase {
  type: "error"
  requestId: string
  agentId: string | null
  message: string
}

/** A scope-wide signpost that clears pending work without mutating completed history. */
export interface WorkforceTruncateEvent extends WorkforceEventBase {
  type: "truncate"
  agentId: string | null
  reason: string | null
}

/** The complete append-only ledger union for workforce runtime replay. */
export type WorkforceLedgerEvent =
  | WorkforceRequestEvent
  | WorkforceHandleEvent
  | WorkforceResponseEvent
  | WorkforceSuspendEvent
  | WorkforceCancelEvent
  | WorkforceUpdateEvent
  | WorkforceErrorEvent
  | WorkforceTruncateEvent

/** The replayed state for one logical workforce request. */
export interface WorkforceRequestRecord {
  id: string
  toAgentId: string
  fromAgentId: string | null
  intent: WorkforceRequestIntent
  input: string
  updates: string[]
  status: WorkforceRequestStatus
  createdAt: string
  updatedAt: string
  attemptCount: number
  activeSessionId: string | null
  response: string | null
  suspendedReason: string | null
  errorMessage: string | null
  cancelledReason: string | null
}

/** Aggregate queue counts exposed to daemon and SDK clients. */
export interface WorkforceProjectionSummary {
  activeRequestCount: number
  queuedRequestCount: number
  suspendedRequestCount: number
  failedRequestCount: number
}

/** The replayed projection used by the runtime to drive queues and summaries. */
export interface WorkforceProjection {
  requests: Record<string, WorkforceRequestRecord>
  queues: Record<string, string[]>
  summary: WorkforceProjectionSummary
}

/** Stable runtime states reported for daemon-managed workforce hosts. */
export type WorkforceRuntimeState = "running"

/** Workforce status summary exposed over daemon IPC. */
export type WorkforceStatus = WorkforceProjectionSummary & {
  state: WorkforceRuntimeState
  rootDir: string
  configPath: string
  ledgerPath: string
}
