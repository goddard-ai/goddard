import type {
  InboxEntityId,
  InboxItem,
  InboxPriority,
  InboxReason,
  InboxStatus,
} from "@goddard-ai/schema/daemon"

export const DEFAULT_INBOX_FILTER_ID = "unread"

export const inboxFilterDefinitions = {
  unread: {
    label: "Unread",
    statuses: ["unread", "read"],
    emptyTitle: "No unread inbox items",
    emptyDescription:
      "Unread and acknowledged items that remain in the active queue will appear here.",
  },
  saved: {
    label: "Saved",
    statuses: ["saved"],
    emptyTitle: "No saved inbox items",
    emptyDescription: "Items you park for later will appear here.",
  },
  replied: {
    label: "Replied",
    statuses: ["replied"],
    emptyTitle: "No replied inbox items",
    emptyDescription: "Items handed back to an agent through a reply will appear here.",
  },
  completed: {
    label: "Completed",
    statuses: ["completed"],
    emptyTitle: "No completed inbox items",
    emptyDescription: "Entity-specific completed items will appear here.",
  },
  archived: {
    label: "Archived",
    statuses: ["archived"],
    emptyTitle: "No archived inbox items",
    emptyDescription: "Items hidden from the active workflow will appear here.",
  },
} as const satisfies Record<
  string,
  {
    label: string
    statuses: readonly InboxStatus[]
    emptyTitle: string
    emptyDescription: string
  }
>

export const inboxFilterOrder = [
  "unread",
  "saved",
  "replied",
  "completed",
  "archived",
] as const satisfies readonly InboxFilterId[]

/** Stable filter ids supported by the inbox primary view. */
export type InboxFilterId = keyof typeof inboxFilterDefinitions

/** Entity families supported by daemon-local inbox rows. */
export type InboxEntityKind = "session" | "pullRequest"

const statusLabels = {
  unread: "Unread",
  read: "Read",
  replied: "Replied",
  completed: "Completed",
  saved: "Saved",
  archived: "Archived",
} satisfies Record<InboxStatus, string>

const priorityLabels = {
  normal: "Normal priority",
  low: "Low priority",
} satisfies Record<InboxPriority, string>

const reasonLabels = {
  "session.blocked": "Session blocked",
  "session.turn_ended": "Session turn ended",
  "pull_request.created": "Pull request created",
  "pull_request.updated": "Pull request updated",
} satisfies Record<InboxReason, string>

function normalizeSearchText(value: string) {
  return value.trim().toLowerCase()
}

function compactText(parts: Array<string | null | undefined>) {
  return parts
    .map((part) => part?.trim() ?? "")
    .filter((part) => part.length > 0)
    .join(" ")
}

/** Returns the daemon entity family for one inbox row id. */
export function getInboxEntityKind(entityId: InboxEntityId) {
  return entityId.startsWith("ses_") ? "session" : "pullRequest"
}

/** Returns the compact user-facing label for one inbox row entity family. */
export function getInboxEntityLabel(entityId: InboxEntityId) {
  return getInboxEntityKind(entityId) === "session" ? "Session" : "Pull request"
}

/** Returns the user-facing label for one inbox workflow status. */
export function getInboxStatusLabel(status: InboxStatus) {
  return statusLabels[status]
}

/** Returns the user-facing label for one inbox priority. */
export function getInboxPriorityLabel(priority: InboxPriority) {
  return priorityLabels[priority]
}

/** Returns the user-facing label for the daemon event that refreshed one inbox row. */
export function getInboxReasonLabel(reason: InboxReason) {
  return reasonLabels[reason]
}

/** Returns the primary row text, preferring the daemon-supplied stable scope. */
export function getInboxItemPrimaryText(item: InboxItem) {
  return item.scope?.trim() || getInboxEntityLabel(item.entityId)
}

/** Returns the secondary row text, preferring the daemon-supplied turn headline. */
export function getInboxItemSecondaryText(item: InboxItem) {
  return item.headline?.trim() || getInboxReasonLabel(item.reason)
}

/** Returns a single searchable row string without creating app-owned inbox data. */
export function getInboxItemSearchText(item: InboxItem) {
  return normalizeSearchText(
    compactText([
      getInboxEntityLabel(item.entityId),
      getInboxStatusLabel(item.status),
      getInboxPriorityLabel(item.priority),
      getInboxReasonLabel(item.reason),
      item.scope,
      item.headline,
      item.entityId,
    ]),
  )
}

/** Filters inbox rows by human-visible row text while preserving daemon order. */
export function filterInboxItemsBySearch(items: readonly InboxItem[], searchQuery: string) {
  const normalizedQuery = normalizeSearchText(searchQuery)

  if (normalizedQuery.length === 0) {
    return items
  }

  return items.filter((item) => getInboxItemSearchText(item).includes(normalizedQuery))
}

/** Formats an inbox row update timestamp for compact list display. */
export function formatInboxUpdatedTime(value: number, now = Date.now()) {
  const diffMinutes = Math.max(0, Math.floor((now - value) / 60000))

  if (diffMinutes < 1) {
    return "now"
  }

  if (diffMinutes < 60) {
    return `${diffMinutes}m`
  }

  const diffHours = Math.floor(diffMinutes / 60)

  if (diffHours < 24) {
    return `${diffHours}h`
  }

  const diffDays = Math.floor(diffHours / 24)

  if (diffDays < 7) {
    return `${diffDays}d`
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(new Date(value))
}
