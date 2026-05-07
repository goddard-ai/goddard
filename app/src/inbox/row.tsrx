import type { InboxItem } from "@goddard-ai/schema/daemon"
import { GitPullRequest, MessageSquare } from "lucide-react"

import {
  formatInboxUpdatedTime,
  getInboxEntityKind,
  getInboxEntityLabel,
  getInboxItemPrimaryText,
  getInboxItemSecondaryText,
  getInboxPriorityLabel,
  getInboxReasonLabel,
  getInboxStatusLabel,
} from "./presentation.ts"
import styles from "./page.style.ts"

export component InboxRow(props: {
  item: InboxItem;
}) {
  const entityKind = getInboxEntityKind(props.item.entityId)
  const EntityIcon =
    entityKind === "session" ? MessageSquare : GitPullRequest
  const priorityLabel = getInboxPriorityLabel(props.item.priority)
  const statusLabel = getInboxStatusLabel(props.item.status)
  const reasonLabel = getInboxReasonLabel(props.item.reason)

  <article class={styles.row} data-inbox-row="true">
    <span class={styles.entityIcon} aria-hidden="true">
      <EntityIcon size={16} strokeWidth={2.1} />
    </span>
    <div class={styles.rowMain}>
      <div class={styles.rowMeta}>
        <span class={styles.entityLabel}>
          {getInboxEntityLabel(props.item.entityId)}
        </span>
        <span class={styles.reasonLabel}>
          {reasonLabel}
        </span>
      </div>
      <div class={styles.rowCopy}>
        <h2 class={styles.rowTitle}>
          {getInboxItemPrimaryText(props.item)}
        </h2>
        <p class={styles.rowDescription}>
          {getInboxItemSecondaryText(props.item)}
        </p>
      </div>
    </div>
    <div class={styles.rowState}>
      <div class={styles.badgeRow}>
        <span class={styles.statusBadge} data-status={props.item.status}>
          {statusLabel}
        </span>
        <span class={styles.priorityBadge} data-priority={props.item.priority}>
          {priorityLabel}
        </span>
      </div>
      <time
        class={styles.updated}
        dateTime={new Date(props.item.updatedAt).toISOString()}
        title={new Intl.DateTimeFormat(undefined, {
          dateStyle: "medium",
          timeStyle: "short",
        }).format(new Date(props.item.updatedAt))}
      >
        {formatInboxUpdatedTime(props.item.updatedAt)}
      </time>
    </div>
  </article>
}
