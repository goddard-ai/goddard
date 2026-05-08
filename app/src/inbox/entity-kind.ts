import type { DaemonPullRequestId, DaemonSessionId } from "@goddard-ai/schema/common/params"
import type { InboxEntityId, InboxItem } from "@goddard-ai/schema/daemon"

/** Entity families supported by daemon-local inbox rows. */
export type InboxEntityKind = "session" | "pullRequest"

/** Inbox row whose linked daemon entity is a session. */
export type SessionInboxItem = InboxItem & {
  entityId: DaemonSessionId
}

/** Inbox row whose linked daemon entity is a pull request. */
export type PullRequestInboxItem = InboxItem & {
  entityId: DaemonPullRequestId
}

/** Returns the daemon entity family for one inbox row id. */
export function getInboxEntityKind(entityId: InboxEntityId) {
  return entityId.startsWith("ses_") ? "session" : "pullRequest"
}

/** Narrows one inbox row to the requested daemon entity family. */
export function isInboxEntityKind(item: InboxItem, kind: "session"): item is SessionInboxItem
export function isInboxEntityKind(
  item: InboxItem,
  kind: "pullRequest",
): item is PullRequestInboxItem
export function isInboxEntityKind(item: InboxItem, kind: InboxEntityKind) {
  return getInboxEntityKind(item.entityId) === kind
}

/** Returns the compact user-facing label for one inbox row entity family. */
export function getInboxEntityLabel(entityId: InboxEntityId) {
  return getInboxEntityKind(entityId) === "session" ? "Session" : "Pull request"
}
