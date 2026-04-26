import { z } from "zod"

const CloudSessionPayload = z.unknown().optional()

/** Stable identifier for one cloud-owned agent session. */
export const CloudSessionId = z
  .string()
  .min(1)
  .max(128)
  .regex(/^[A-Za-z0-9][A-Za-z0-9_.:-]*$/)

export type CloudSessionId = z.infer<typeof CloudSessionId>

/** Lifecycle state for the coordinator's view of one cloud session. */
export const CloudSessionStatus = z.enum(["creating", "running", "idle", "failed", "ended"])

export type CloudSessionStatus = z.infer<typeof CloudSessionStatus>

/** Lifecycle state for the sandbox that hosts the ACP harness. */
export const CloudSandboxStatus = z.enum([
  "pending",
  "starting",
  "ready",
  "disconnected",
  "failed",
  "stopped",
])

export type CloudSandboxStatus = z.infer<typeof CloudSandboxStatus>

/** Current cloud-session state projected from the coordinator's event log. */
export const CloudSessionSnapshot = z.object({
  id: CloudSessionId,
  status: CloudSessionStatus,
  sandboxStatus: CloudSandboxStatus,
  harnessEpoch: z.number().int().nonnegative(),
  lastSeq: z.number().int().nonnegative(),
  createdAt: z.string(),
  updatedAt: z.string(),
  metadata: z.record(z.string(), z.unknown()).optional(),
})

export type CloudSessionSnapshot = z.infer<typeof CloudSessionSnapshot>

/** Append-only fact emitted by the cloud coordinator for daemon synchronization. */
export const CloudSessionEvent = z.object({
  seq: z.number().int().positive(),
  at: z.string(),
  source: z.enum(["coordinator", "harness", "local-daemon"]),
  type: z.string().min(1),
  payload: CloudSessionPayload,
})

export type CloudSessionEvent = z.infer<typeof CloudSessionEvent>

/** Request body for creating or rehydrating a cloud-owned session. */
export const CreateCloudSessionRequest = z.object({
  sessionId: CloudSessionId.optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
})

export type CreateCloudSessionRequest = z.infer<typeof CreateCloudSessionRequest>

/** Response returned when a cloud session is created. */
export const CreateCloudSessionResponse = z.object({
  session: CloudSessionSnapshot,
  events: z.array(CloudSessionEvent),
})

export type CreateCloudSessionResponse = z.infer<typeof CreateCloudSessionResponse>

/** Command accepted from a local daemon and forwarded to the cloud harness. */
export const CloudSessionCommand = z.object({
  commandId: z.string().min(1).max(128),
  type: z.enum(["initialize", "prompt", "cancel", "shutdown", "custom"]),
  payload: CloudSessionPayload,
})

export type CloudSessionCommand = z.infer<typeof CloudSessionCommand>

/** Response returned after enqueueing a command for the cloud harness. */
export const CloudSessionCommandResponse = z.object({
  commandId: z.string(),
  duplicate: z.boolean(),
  accepted: z.boolean(),
  event: CloudSessionEvent.optional(),
})

export type CloudSessionCommandResponse = z.infer<typeof CloudSessionCommandResponse>

/** Harness-originated message sent from the ACP sandbox to the coordinator. */
export const CloudSessionHarnessMessage = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("event"),
    eventType: z.string().min(1),
    payload: CloudSessionPayload,
  }),
  z.object({
    type: z.literal("status"),
    status: CloudSessionStatus,
    sandboxStatus: CloudSandboxStatus.optional(),
    detail: z.string().optional(),
  }),
  z.object({
    type: z.literal("error"),
    message: z.string().min(1),
    payload: CloudSessionPayload,
  }),
])

export type CloudSessionHarnessMessage = z.infer<typeof CloudSessionHarnessMessage>

/** Response returned when a daemon consumes coordinator events. */
export const CloudSessionSyncResponse = z.object({
  session: CloudSessionSnapshot,
  events: z.array(CloudSessionEvent),
  cursor: z.number().int().nonnegative(),
  hasMore: z.boolean(),
})

export type CloudSessionSyncResponse = z.infer<typeof CloudSessionSyncResponse>
