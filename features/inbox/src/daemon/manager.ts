import { IpcClientError } from "@goddard-ai/ipc"
import type { SessionId } from "@goddard-ai/session/schema"
import type { KindInput } from "kindstore"

import type { InboxStore } from "../daemon.ts"
import type {
  BulkUpdateInboxItemsRequest,
  InboxEntityId,
  InboxHeadline,
  InboxItem,
  InboxItemEventMutation,
  InboxPriority,
  InboxReason,
  InboxScope,
  InboxStatus,
  ListInboxRequest,
  UpdateInboxItemRequest,
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

function normalizeInboxPageSize(limit?: number) {
  if (!Number.isFinite(limit)) {
    return DEFAULT_INBOX_PAGE_SIZE
  }

  return Math.min(Math.max(Math.trunc(limit ?? DEFAULT_INBOX_PAGE_SIZE), 1), MAX_INBOX_PAGE_SIZE)
}

function isSessionEntityId(entityId: InboxEntityId): entityId is SessionId {
  return entityId.startsWith("ses_")
}

function assertUserWorkflowStatus(entityId: InboxEntityId, status: InboxStatus | undefined) {
  if (!status) {
    return
  }

  if (!userWorkflowStatuses.has(status)) {
    throw new IpcClientError("Inbox status completed requires an entity-specific operation")
  }

  if (status === "replied" && !isSessionEntityId(entityId)) {
    throw new IpcClientError("Inbox status replied only applies to session entities")
  }
}

function assertMutableFields(input: { status?: InboxStatus; priority?: InboxPriority }) {
  if (!input.status && !input.priority) {
    throw new IpcClientError("At least one inbox field must be updated")
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
      throw new IpcClientError("Inbox status filter cannot be empty")
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
      throw new IpcClientError("Invalid inbox cursor")
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
      throw new IpcClientError("Inbox item not found")
    }

    const item = db.inboxItems.update(existing.id, {
      ...withWorkflowStatus(existing, input.status, timestamp),
      ...(input.priority && { priority: input.priority }),
      updatedAt: timestamp,
    })
    if (!item) {
      throw new IpcClientError("Inbox item not found")
    }

    return { item: publishItem(item, "updated") }
  }

  function bulkUpdateInboxItems(input: BulkUpdateInboxItemsRequest) {
    assertMutableFields(input)
    if (input.entityIds.length === 0) {
      throw new IpcClientError("Inbox bulk update requires at least one entity id")
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
      throw new IpcClientError("Inbox item not found")
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
      throw new IpcClientError("Inbox item not found")
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
