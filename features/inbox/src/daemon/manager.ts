import {
  IpcClientError,
  type IpcClientErrorPayload,
  type IpcErrorDescriptorForCode,
  type IpcErrorDetails,
} from "@goddard-ai/ipc"
import type { SessionId } from "@goddard-ai/session/schema"
import type { KindInput } from "kindstore"

import type { InboxStore } from "../daemon.ts"
import {
  InboxErrorCodes,
  InboxIpcErrors,
  type BulkUpdateInboxItemsRequest,
  type InboxEntityId,
  type InboxErrorCode,
  type InboxHeadline,
  type InboxItem,
  type InboxItemEventMutation,
  type InboxPriority,
  type InboxReason,
  type InboxScope,
  type InboxStatus,
  type ListInboxRequest,
  type UpdateInboxItemRequest,
} from "../schema.ts"

const DEFAULT_INBOX_PAGE_SIZE = 50
const MAX_INBOX_PAGE_SIZE = 100
const userWorkflowStatuses = new Set<InboxStatus>([
  "unread",
  "read",
  "replied",
  "saved",
  "archived",
])

type InboxItemInput = KindInput<InboxStore["schema"]["inboxItems"]>

type TouchInboxItemInput = {
  entityId: InboxEntityId
  reason: InboxReason
  priority?: InboxPriority
  scope?: InboxScope | null
  headline?: InboxHeadline | null
  turnId?: string | null
}

type InboxItemEventPublisher = (payload: {
  item: InboxItem
  mutation: InboxItemEventMutation
}) => void

type InboxManagerOptions = {
  db: InboxStore
  publishEvent: InboxItemEventPublisher
}

type InboxIpcErrorDescriptor<TCode extends InboxErrorCode> = IpcErrorDescriptorForCode<
  typeof InboxIpcErrors,
  TCode
>

function normalizeInboxPageSize(limit?: number) {
  if (!Number.isFinite(limit)) {
    return DEFAULT_INBOX_PAGE_SIZE
  }

  return Math.min(Math.max(Math.trunc(limit ?? DEFAULT_INBOX_PAGE_SIZE), 1), MAX_INBOX_PAGE_SIZE)
}

function isSessionEntityId(entityId: InboxEntityId): entityId is SessionId {
  return entityId.startsWith("ses_")
}

function createInboxIpcError<TCode extends InboxErrorCode>(
  code: TCode,
  ...[details]: undefined extends IpcErrorDetails<InboxIpcErrorDescriptor<TCode>>
    ? [details?: IpcErrorDetails<InboxIpcErrorDescriptor<TCode>>]
    : [details: IpcErrorDetails<InboxIpcErrorDescriptor<TCode>>]
) {
  const input = {
    code,
    ...(details === undefined ? {} : { details }),
  } as IpcClientErrorPayload<InboxIpcErrorDescriptor<TCode>>

  return new IpcClientError<InboxIpcErrorDescriptor<TCode>>(input)
}

function assertUserWorkflowStatus(entityId: InboxEntityId, status: InboxStatus | undefined) {
  if (!status) {
    return
  }

  if (!userWorkflowStatuses.has(status)) {
    throw createInboxIpcError(InboxErrorCodes.CompletedRequiresEntityOperation, {
      entityId,
      status,
    })
  }

  if (status === "replied" && !isSessionEntityId(entityId)) {
    throw createInboxIpcError(InboxErrorCodes.RepliedRequiresSessionEntity, { entityId, status })
  }
}

function assertMutableFields(input: { status?: InboxStatus; priority?: InboxPriority }) {
  if (!input.status && !input.priority) {
    throw createInboxIpcError(InboxErrorCodes.EmptyUpdate)
  }
}

function withWorkflowStatus(
  item: Pick<InboxItemInput, "readAt">,
  status: InboxStatus | undefined,
  timestamp: number,
) {
  if (!status) {
    return {}
  }

  return {
    status,
    readAt: status === "read" ? timestamp : status === "unread" ? null : item.readAt,
  } satisfies Partial<InboxItemInput>
}

/** Creates the daemon-owned inbox manager that centralizes all inbox writes. */
export function createInboxManager(options: InboxManagerOptions) {
  const { db } = options

  function publishItem(item: InboxItem, mutation: InboxItemEventMutation) {
    options.publishEvent({ item, mutation })
    return item
  }

  function listInboxItems(params: ListInboxRequest) {
    const pageSize = normalizeInboxPageSize(params.limit)
    const statuses = params.statuses ?? ["unread"]
    if (statuses.length === 0) {
      throw createInboxIpcError(InboxErrorCodes.EmptyStatusFilter)
    }

    let page: ReturnType<typeof db.inboxItems.findPage>
    try {
      page = db.inboxItems.findPage({
        where: {
          status: { in: statuses },
        },
        orderBy: {
          updatedAt: "desc",
          id: "desc",
        },
        limit: pageSize,
        after: params.cursor ?? undefined,
      })
    } catch {
      throw createInboxIpcError(InboxErrorCodes.InvalidCursor, {
        cursor: params.cursor ?? null,
      })
    }

    return {
      items: page.items,
      nextCursor: page.next ?? null,
      hasMore: page.next != null,
    }
  }

  function touchInboxItem(input: TouchInboxItemInput) {
    const timestamp = Date.now()
    const existing =
      db.inboxItems.first({
        where: { entityId: input.entityId },
      }) ?? null
    const nextItem: InboxItemInput = {
      entityId: input.entityId,
      reason: input.reason,
      status: "unread",
      priority: input.priority ?? existing?.priority ?? "normal",
      updatedAt: timestamp,
      readAt: null,
      scope: input.scope ?? existing?.scope ?? null,
      headline: input.headline ?? existing?.headline ?? null,
      turnId: input.turnId ?? existing?.turnId ?? null,
    }

    return publishItem(db.inboxItems.putByUnique({ entityId: input.entityId }, nextItem), "touched")
  }

  function updateInboxItem(input: UpdateInboxItemRequest) {
    assertMutableFields(input)
    assertUserWorkflowStatus(input.entityId, input.status)
    const timestamp = Date.now()
    const existing =
      db.inboxItems.first({
        where: { entityId: input.entityId },
      }) ?? null
    if (!existing) {
      throw createInboxIpcError(InboxErrorCodes.ItemNotFound, {
        entityId: input.entityId,
      })
    }

    const item = db.inboxItems.update(existing.id, {
      ...withWorkflowStatus(existing, input.status, timestamp),
      ...(input.priority && { priority: input.priority }),
      updatedAt: timestamp,
    })
    if (!item) {
      throw createInboxIpcError(InboxErrorCodes.ItemNotFound, {
        entityId: input.entityId,
      })
    }

    return { item: publishItem(item, "updated") }
  }

  function bulkUpdateInboxItems(input: BulkUpdateInboxItemsRequest) {
    assertMutableFields(input)
    if (input.entityIds.length === 0) {
      throw createInboxIpcError(InboxErrorCodes.EmptyBulkUpdate)
    }

    const entityIds = [...new Set(input.entityIds)]
    for (const entityId of entityIds) {
      assertUserWorkflowStatus(entityId, input.status)
    }

    const timestamp = Date.now()
    const items: InboxItem[] = []
    const missingEntityIds: InboxEntityId[] = []

    db.batch(() => {
      for (const entityId of entityIds) {
        const existing =
          db.inboxItems.first({
            where: { entityId },
          }) ?? null
        if (!existing) {
          missingEntityIds.push(entityId)
          continue
        }

        const item = db.inboxItems.update(existing.id, {
          ...withWorkflowStatus(existing, input.status, timestamp),
          ...(input.priority && { priority: input.priority }),
          updatedAt: timestamp,
        })
        if (item) {
          items.push(publishItem(item, "bulk_updated"))
        }
      }
    })

    return {
      items,
      missingEntityIds,
    }
  }

  function markSessionReplied(sessionId: SessionId) {
    const existing =
      db.inboxItems.first({
        where: { entityId: sessionId },
      }) ?? null
    if (!existing || existing.status === "archived") {
      return null
    }

    const item = db.inboxItems.update(existing.id, {
      status: "replied",
      updatedAt: Date.now(),
    })
    if (!item) {
      throw createInboxIpcError(InboxErrorCodes.ItemNotFound, {
        entityId: sessionId,
      })
    }

    return publishItem(item, "replied")
  }

  function completeSession(sessionId: SessionId) {
    const existing =
      db.inboxItems.first({
        where: { entityId: sessionId },
      }) ?? null
    if (!existing) {
      return null
    }

    const item = db.inboxItems.update(existing.id, {
      status: "completed",
      updatedAt: Date.now(),
    })
    if (!item) {
      throw createInboxIpcError(InboxErrorCodes.ItemNotFound, {
        entityId: sessionId,
      })
    }

    return publishItem(item, "completed")
  }

  return {
    listInboxItems,
    touchInboxItem,
    updateInboxItem,
    bulkUpdateInboxItems,
    markSessionReplied,
    completeSession,
  }
}

export type InboxManager = ReturnType<typeof createInboxManager>
