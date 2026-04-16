import { z } from "zod"

import type { WorkforceConfig, WorkforceLedgerEvent, WorkforceStatus } from "../workforce.ts"

/** Stable request intents supported by workforce mutation APIs. */
export const WorkforceRequestIntent = z.enum(["default", "create"])

export type WorkforceRequestIntent = z.infer<typeof WorkforceRequestIntent>

/** Optional daemon-issued workforce continuation token. */
export const WorkforceToken = z.string().optional()

export type WorkforceToken = z.infer<typeof WorkforceToken>

/** Request payload used to start one daemon-owned workforce runtime. */
export const StartWorkforceRequest = z.strictObject({
  rootDir: z.string(),
})

export type StartWorkforceRequest = z.infer<typeof StartWorkforceRequest>

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

/** Request payload used to subscribe to live daemon-published workforce events for one repo. */
export const SubscribeWorkforceEventsRequest = z.strictObject({
  rootDir: z.string(),
})

/** Compile-time shape used to subscribe to live daemon-published workforce events for one repo. */
export type SubscribeWorkforceEventsRequest = z.infer<typeof SubscribeWorkforceEventsRequest>

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

/** Stream payload emitted for one workforce ledger event from one active repository runtime. */
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
