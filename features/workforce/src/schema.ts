import { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { SessionId } from "@goddard-ai/session/schema"
import { z } from "zod"

/** Root config defaults used when initializing repository-local workforce files. */
export const WorkforceRootConfig = z
  .strictObject({
    defaultAgent: z
      .union([z.string().min(1), AgentDistribution])
      .optional()
      .describe("Default agent written into newly initialized repository workforce config."),
  })
  .describe("Workforce defaults loaded from Goddard root config.")

export type WorkforceRootConfig = z.infer<typeof WorkforceRootConfig>

/** Stable request intents supported by workforce mutation APIs. */
export const WorkforceRequestIntent = z.enum(["default", "create"])

export type WorkforceRequestIntent = z.infer<typeof WorkforceRequestIntent>

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

/** Optional daemon-issued workforce continuation token. */
export const WorkforceToken = z.string().optional()

export type WorkforceToken = z.infer<typeof WorkforceToken>

/** Request payload used to start one daemon-owned workforce runtime. */
export const StartWorkforceRequest = z.strictObject({
  rootDir: z.string(),
})

export type StartWorkforceRequest = z.infer<typeof StartWorkforceRequest>

/** Persisted workforce attachment owned by the workforce feature. */
export const DaemonWorkforce = z.strictObject({
  sessionId: SessionId,
  rootDir: z.string().optional(),
  agentId: z.string().optional(),
  requestId: z.string().optional(),
})

export type DaemonWorkforce = z.output<typeof DaemonWorkforce> & {
  id: `wf_${string}`
}

/** Response payload returned after one daemon-managed session workforce fetch. */
export type GetSessionWorkforceResponse = {
  id: SessionId
  acpSessionId: string
  workforce: DaemonWorkforce | null
}

/** One package candidate that can become a workforce domain during initialization. */
export type WorkforceInitCandidate = {
  rootDir: string
  relativeDir: string
  manifestPath: string
  name: string
}

/** One initialized workforce file set created under a repository root. */
export type InitializedWorkforce = {
  rootDir: string
  configPath: string
  ledgerPath: string
  createdPaths: string[]
}

/** Request payload used to discover workforce initialization candidates for one repository. */
export const DiscoverWorkforceCandidatesRequest = z.strictObject({
  rootDir: z.string(),
})

export type DiscoverWorkforceCandidatesRequest = z.infer<typeof DiscoverWorkforceCandidatesRequest>

/** Request payload used to initialize one repository workforce config and ledger. */
export const InitializeWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  packageDirs: z.array(z.string()),
})

export type InitializeWorkforceRequest = z.infer<typeof InitializeWorkforceRequest>

/** Request payload used to fetch one daemon-owned workforce runtime. */
export const GetWorkforceRequest = z.strictObject({
  rootDir: z.string(),
})

export type GetWorkforceRequest = z.infer<typeof GetWorkforceRequest>

/** Response payload returned after workforce initialization candidates are discovered. */
export type DiscoverWorkforceCandidatesResponse = {
  rootDir: string
  candidates: WorkforceInitCandidate[]
}

/** Response payload returned after one repository workforce is initialized. */
export type InitializeWorkforceResponse = {
  initialized: InitializedWorkforce
}

/** Request payload used to stop one daemon-owned workforce runtime. */
export const ShutdownWorkforceRequest = z.strictObject({
  rootDir: z.string(),
})

export type ShutdownWorkforceRequest = z.infer<typeof ShutdownWorkforceRequest>

/** Request payload used to enqueue work for one target workforce agent. */
export const CreateWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  targetAgentId: z.string(),
  input: z.string(),
  intent: WorkforceRequestIntent.optional(),
  token: WorkforceToken,
})

export type CreateWorkforceRequest = z.infer<typeof CreateWorkforceRequest>

/** Request payload used to add resume context to one workforce request. */
export const UpdateWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  requestId: z.string(),
  input: z.string(),
  token: WorkforceToken,
})

export type UpdateWorkforceRequest = z.infer<typeof UpdateWorkforceRequest>

/** Request payload used to cancel one workforce request. */
export const CancelWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  requestId: z.string(),
  reason: z.string().optional(),
  token: WorkforceToken,
})

export type CancelWorkforceRequest = z.infer<typeof CancelWorkforceRequest>

/** Request payload used to clear pending work in one agent scope or the whole runtime. */
export const TruncateWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  agentId: z.string().optional(),
  reason: z.string().optional(),
  token: WorkforceToken,
})

export type TruncateWorkforceRequest = z.infer<typeof TruncateWorkforceRequest>

/** Request payload used by an active workforce agent to finish its current task. */
export const RespondWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  output: z.string(),
  token: z.string(),
})

export type RespondWorkforceRequest = z.infer<typeof RespondWorkforceRequest>

/** Request payload used by an active workforce agent to suspend its current task. */
export const SuspendWorkforceRequest = z.strictObject({
  rootDir: z.string(),
  reason: z.string(),
  token: z.string(),
})

export type SuspendWorkforceRequest = z.infer<typeof SuspendWorkforceRequest>

/** Internal routing envelope for one workforce ledger event from one active repository runtime. */
export interface WorkforceEventEnvelope {
  rootDir: string
  event: WorkforceLedgerEvent
}

/** One daemon-managed workforce runtime addressed by repository root. */
export type WorkforceDescription = WorkforceStatus & {
  config: WorkforceConfig
}

/** Response payload returned when one workforce runtime is fetched. */
export type GetWorkforceResponse = {
  workforce: WorkforceDescription
}

/** Response payload returned when one workforce runtime is started. */
export type StartWorkforceResponse = {
  workforce: WorkforceDescription
}

/** Response payload returned when all running workforce runtimes are listed. */
export type ListWorkforcesResponse = {
  workforces: WorkforceStatus[]
}

/** Response payload returned after one workforce runtime is stopped. */
export type ShutdownWorkforceResponse = {
  rootDir: string
  success: boolean
}

/** Response payload returned after one workforce request mutation. */
export type MutateWorkforceResponse = {
  workforce: WorkforceStatus
  requestId: string | null
}
