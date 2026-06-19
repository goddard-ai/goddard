import { plural, t } from "@lingui/core/macro"
import { Archive, ArrowDown, ArrowUp, Bookmark, Mail, MailOpen, X } from "lucide-react"

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

  <div class={styles.bulkToolbar} aria-label={t`Selected inbox item actions`} role="toolbar">
    <span class={styles.bulkCount}>{plural(props.selectedCount, {
      one: "# selected",
      other: "# selected",
    })}</span>
    <div class={styles.bulkActionGroup}>
      <InboxBulkToolbarButton
        ariaLabel={t`Mark selected read`}
        icon={MailOpen}
        onPress={() => {
          props.onMarkRead()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={t`Mark selected unread`}
        icon={Mail}
        onPress={() => {
          props.onMarkUnread()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={t`Save selected`}
        icon={Bookmark}
        onPress={() => {
          props.onSave()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={t`Set selected low priority`}
        icon={ArrowDown}
        onPress={() => {
          props.onSetLowPriority()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={t`Set selected normal priority`}
        icon={ArrowUp}
        onPress={() => {
          props.onSetNormalPriority()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={t`Archive selected`}
        icon={Archive}
        onPress={() => {
          props.onArchive()
        }}
      />
      <InboxBulkToolbarButton
        ariaLabel={t`Clear selection`}
        icon={X}
        onPress={() => {
          props.onClear()
        }}
      />
    </div>
  </div>
}
