import { Archive, ArrowDown, ArrowUp, Bookmark, Mail, MailOpen, X } from "lucide-react"

import { text } from "~/language/text.ts"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import styles from "./page.style.ts"

function InboxBulkToolbarButton(props: {
  ariaLabel: string;
  icon: typeof Archive;
  onPress: () => void;
}) @{
  const Icon = props.icon

  <GoodTooltip content={props.ariaLabel}>
    <button
      aria-label={props.ariaLabel}
      class={styles.bulkActionButton}
      title={props.ariaLabel}
      type="button"
      onClick={() => {
        props.onPress()
      }}
    >
      <Icon aria-hidden={true} size={15} strokeWidth={2.2} />
    </button>
  </GoodTooltip>
}

export function InboxBulkToolbar(props: {
  selectedCount: number;
  onArchive: () => void;
  onClear: () => void;
  onMarkRead: () => void;
  onMarkUnread: () => void;
  onSave: () => void;
  onSetLowPriority: () => void;
  onSetNormalPriority: () => void;
}) @{
  if (props.selectedCount === 0) {
    return null
  }

  <div class={styles.bulkToolbar} aria-label={text.selectedInboxItemActions} role="toolbar">
    <span class={styles.bulkCount}>{`${props.selectedCount} ${text.selectedCount}`}</span>
    <div class={styles.bulkActionGroup}>
      <InboxBulkToolbarButton
        ariaLabel={text.markSelectedRead}
        icon={MailOpen}
        onPress={() => {
          props.onMarkRead()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={text.markSelectedUnread}
        icon={Mail}
        onPress={() => {
          props.onMarkUnread()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={text.saveSelected}
        icon={Bookmark}
        onPress={() => {
          props.onSave()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={text.setSelectedLowPriority}
        icon={ArrowDown}
        onPress={() => {
          props.onSetLowPriority()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={text.setSelectedNormalPriority}
        icon={ArrowUp}
        onPress={() => {
          props.onSetNormalPriority()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={text.archiveSelected}
        icon={Archive}
        onPress={() => {
          props.onArchive()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={text.clearSelection}
        icon={X}
        onPress={() => {
          props.onClear()
        }}
      />
    </div>
  </div>
}
