import { IpcClientError } from "@goddard-ai/ipc"
import type { DaemonPullRequestId, DaemonSessionId } from "@goddard-ai/schema/common/params"
import type {
  BulkUpdateInboxItemsRequest,
  InboxEntityId,
  InboxHeadline,
  InboxItem,
  InboxPriority,
  InboxReason,
  InboxScope,
  InboxStatus,
  ListInboxRequest,
  UpdateInboxItemRequest,
} from "@goddard-ai/schema/daemon"
import type { KindInput } from "kindstore"

import { db } from "../persistence/store.ts"

const DEFAULT_INBOX_PAGE_SIZE = 50
const MAX_INBOX_PAGE_SIZE = 100
const userWorkflowStatuses = new Set<InboxStatus>([
  "unread",
  "read",
  "replied",
  "saved",
  "archived",
])

type InboxItemInput = KindInput<typeof db.schema.inboxItems>

type TouchInboxItemInput = {
  entityId: InboxEntityId
  reason: InboxReason
  priority?: InboxPriority
  scope?: InboxScope | null
  headline?: InboxHeadline | null
  turnId?: string | null
}

function now() {
  return Date.now()
}

function normalizeInboxPageSize(limit?: number) {
  if (!Number.isFinite(limit)) {
    return DEFAULT_INBOX_PAGE_SIZE
  }

  return Math.min(Math.max(Math.trunc(limit ?? DEFAULT_INBOX_PAGE_SIZE), 1), MAX_INBOX_PAGE_SIZE)
}

function isSessionEntityId(entityId: InboxEntityId): entityId is DaemonSessionId {
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

function toInboxItem(item: InboxItem): InboxItem {
  return item
}

/** Creates the daemon-owned inbox manager that centralizes all inbox writes. */
export function createInboxManager() {
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
      items: page.items.map(toInboxItem),
      nextCursor: page.next ?? null,
      hasMore: page.next != null,
    }
  }

  function touchInboxItem(input: TouchInboxItemInput) {
    const timestamp = now()
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

    return toInboxItem(db.inboxItems.putByUnique({ entityId: input.entityId }, nextItem))
  }

  function updateInboxItem(input: UpdateInboxItemRequest) {
    assertMutableFields(input)
    assertUserWorkflowStatus(input.entityId, input.status)
    const timestamp = now()
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

    return { item: toInboxItem(item) }
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

    const timestamp = now()
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
          items.push(toInboxItem(item))
        }
      }
    })

    return {
      items,
      missingEntityIds,
    }
  }

  function markSessionReplied(sessionId: DaemonSessionId) {
    const existing =
      db.inboxItems.first({
        where: { entityId: sessionId },
      }) ?? null
    if (!existing || existing.status === "archived") {
      return null
    }

    return toInboxItem(
      db.inboxItems.update(existing.id, {
        status: "replied",
        updatedAt: now(),
      })!,
    )
  }

  function completeSession(sessionId: DaemonSessionId) {
    const existing =
      db.inboxItems.first({
        where: { entityId: sessionId },
      }) ?? null
    if (!existing) {
      return null
    }

    return toInboxItem(
      db.inboxItems.update(existing.id, {
        status: "completed",
        updatedAt: now(),
      })!,
    )
  }

  function getPullRequest(id: DaemonPullRequestId) {
    const pullRequest = db.pullRequests.get(id) ?? null
    if (!pullRequest) {
      throw new IpcClientError("Pull request not found")
    }

    return pullRequest
  }

  return {
    listInboxItems,
    touchInboxItem,
    updateInboxItem,
    bulkUpdateInboxItems,
    markSessionReplied,
    completeSession,
    getPullRequest,
  }
}

export type InboxManager = ReturnType<typeof createInboxManager>
