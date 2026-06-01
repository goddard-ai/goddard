import type { InboxPriority, InboxReason, InboxStatus } from "@goddard-ai/inbox/schema"

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
