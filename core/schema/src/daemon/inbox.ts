import { z } from "zod"

import { DaemonPullRequestId, DaemonSessionId } from "../common/params.ts"

/** Tagged inbox item id emitted by the daemon inbox store. */
export const InboxItemId = z.custom<`inb_${string}`>(
  (value): value is `inb_${string}` => typeof value === "string" && value.startsWith("inb_"),
)

export type InboxItemId = z.infer<typeof InboxItemId>

/** Tagged daemon entity ids that can own one local inbox row. */
export const InboxEntityId = z.union([DaemonSessionId, DaemonPullRequestId])

export type InboxEntityId = z.infer<typeof InboxEntityId>

/** Current workflow state for one local inbox row. */
export const InboxStatus = z.enum(["unread", "read", "replied", "completed", "saved", "archived"])

export type InboxStatus = z.infer<typeof InboxStatus>

/** Coarse inbox priority used for daemon and user sorting decisions. */
export const InboxPriority = z.enum(["normal", "low"])

export type InboxPriority = z.infer<typeof InboxPriority>

/** Daemon-owned event category that last caused an inbox row to need attention. */
export const InboxReason = z.enum([
  "session.blocked",
  "session.turn_ended",
  "pull_request.created",
  "pull_request.updated",
])

export type InboxReason = z.infer<typeof InboxReason>

/** Short semi-stable subject label rendered as the inbox row scope. */
export const InboxScope = z.string().trim().min(1).max(80)

export type InboxScope = z.infer<typeof InboxScope>

/** Short turn-specific inbox preview rendered after the row scope. */
export const InboxHeadline = z.string().trim().min(1).max(120)

export type InboxHeadline = z.infer<typeof InboxHeadline>

/** Optional agent-supplied inbox metadata attached to session turn reporting. */
export const SessionInboxMetadataInput = z.strictObject({
  scope: InboxScope.optional(),
  headline: InboxHeadline.optional(),
})

export type SessionInboxMetadataInput = z.infer<typeof SessionInboxMetadataInput>

/** Current inbox row projection returned by daemon IPC and SDK calls. */
export const InboxItem = z.strictObject({
  id: InboxItemId,
  entityId: InboxEntityId,
  reason: InboxReason,
  status: InboxStatus,
  priority: InboxPriority,
  updatedAt: z.number().int(),
  readAt: z.number().int().nullable().default(null),
  scope: InboxScope.nullable().default(null),
  headline: InboxHeadline.nullable().default(null),
  turnId: z.string().nullable().default(null),
})

export type InboxItem = z.infer<typeof InboxItem>

/** Request payload used to list one page of daemon-local inbox rows. */
export const ListInboxRequest = z.strictObject({
  statuses: z.array(InboxStatus).optional(),
  limit: z.number().int().positive().optional(),
  cursor: z.string().optional(),
})

export type ListInboxRequest = z.infer<typeof ListInboxRequest>

/** Response payload returned after listing one page of inbox rows. */
export type ListInboxResponse = {
  items: InboxItem[]
  nextCursor: string | null
  hasMore: boolean
}

/** Request payload used to update one existing inbox row by entity id. */
export const UpdateInboxItemRequest = z.strictObject({
  entityId: InboxEntityId,
  status: InboxStatus.optional(),
  priority: InboxPriority.optional(),
})

export type UpdateInboxItemRequest = z.infer<typeof UpdateInboxItemRequest>

/** Response payload returned after updating one inbox row. */
export type UpdateInboxItemResponse = {
  item: InboxItem
}

/** Request payload used to update many existing inbox rows with one timestamp. */
export const BulkUpdateInboxItemsRequest = z.strictObject({
  entityIds: z.array(InboxEntityId),
  status: InboxStatus.optional(),
  priority: InboxPriority.optional(),
})

export type BulkUpdateInboxItemsRequest = z.infer<typeof BulkUpdateInboxItemsRequest>

/** Response payload returned after bulk-updating inbox rows. */
export type BulkUpdateInboxItemsResponse = {
  items: InboxItem[]
  missingEntityIds: InboxEntityId[]
}
