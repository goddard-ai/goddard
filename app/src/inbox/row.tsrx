import type { InboxItem } from "@goddard-ai/schema/daemon"
import { GitPullRequest, MessageSquare } from "lucide-react"

import { useMutations } from "~/lib/mutations-provider.tsx"
import { getInboxEntityLabel, isInboxEntityKind } from "./entity-kind.ts"
import { formatInboxUpdatedTime } from "./format.ts"
import { getInboxPriorityLabel, getInboxReasonLabel, getInboxStatusLabel } from "./labels.ts"
import { InboxPageMutations } from "./mutations.ts"
import styles from "./page.style.ts"
import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "./text.ts"

function handleButtonLikeKeyDown(
  event: preact.TargetedKeyboardEvent<HTMLElement>,
  onPress: () => void,
) {
  if (event.key !== "Enter" && event.key !== " ") {
    return
  }

  event.preventDefault()
  onPress()
}

export component InboxRow(props: {
  item: InboxItem;
}) {
  const { openInboxItem } = useMutations(InboxPageMutations)
  const EntityIcon = isInboxEntityKind(props.item, "session") ? MessageSquare : GitPullRequest
  const priorityLabel = getInboxPriorityLabel(props.item.priority)
  const statusLabel = getInboxStatusLabel(props.item.status)
  const reasonLabel = getInboxReasonLabel(props.item.reason)

  <div
    class={styles.row}
    data-inbox-row="true"
    role="button"
    tabIndex={0}
    onClick={() => {
      openInboxItem(props.item)
    }}
    onKeyDown={(event) => {
      handleButtonLikeKeyDown(event, () => openInboxItem(props.item))
    }}
  >
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
  </div>
}
