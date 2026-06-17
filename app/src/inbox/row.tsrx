import type { InboxItem } from "@goddard-ai/inbox/schema"
import { cx } from "@goddard-ai/styled-system/css"
import { useState } from "preact/hooks"
import {
  Archive,
  ArrowDown,
  ArrowUp,
  Bookmark,
  CheckCircle2,
  GitPullRequest,
  Mail,
  MailOpen,
  MessageSquare,
} from "lucide-react"

import { text } from "~/language/text.ts"
import { appToaster } from "~/lib/good-toaster.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { useMutations } from "~/lib/mutations-provider.tsx"
import { getInboxEntityLabel, isInboxEntityKind } from "./entity-kind.ts"
import { formatInboxUpdatedTime } from "./format.ts"
import { getInboxPriorityLabel, getInboxReasonLabel, getInboxStatusLabel } from "./labels.ts"
import { InboxPageMutations } from "./mutations.ts"
import styles from "./page.style.ts"
import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "./text.ts"

function InboxRowActionButton(props: {
  ariaLabel: string;
  disabled?: boolean;
  icon: typeof Archive;
  isActive?: boolean;
  onPress: () => void;
}) @{
  const Icon = props.icon

  <GoodTooltip content={props.ariaLabel}>
    <button
      aria-label={props.ariaLabel}
      class={styles.rowActionButton}
      data-active={props.isActive ? "true" : undefined}
      disabled={props.disabled}
      title={props.ariaLabel}
      type="button"
      onClick={(event) => {
        event.stopPropagation()
        props.onPress()
      }}
    >
      <Icon aria-hidden={true} size={14} strokeWidth={2.2} />
    </button>
  </GoodTooltip>
}

function handleRowOpenKeyDown(
  event: preact.TargetedKeyboardEvent<HTMLElement>,
  onPress: () => void,
) {
  if (event.key !== "Enter" && event.key !== " ") {
    return
  }

  event.preventDefault()
  onPress()
}

export function InboxRow(props: {
  itemRef?: (element: HTMLElement | null) => void | (() => void);
  item: InboxItem;
  isSelected?: boolean;
  showProjectName?: boolean;
  onSelectionChange?: (isSelected: boolean) => void;
}) @{
  const mutations = useMutations(InboxPageMutations)
  const [pendingAction, setPendingAction] = useState<string | null>(null)
  const sessionItem = isInboxEntityKind(props.item, "session") ? props.item : null
  const EntityIcon = sessionItem ? MessageSquare : GitPullRequest
  const priorityLabel = getInboxPriorityLabel(props.item.priority)
  const readStatus =
    props.item.status === "unread" ? "read" : "unread"
  const readLabel =
    props.item.status === "unread" ? text.markRead : text.markUnread
  const priority =
    props.item.priority === "low" ? "normal" : "low"
  const priorityActionLabel =
    props.item.priority === "low" ? text.setNormalPriority : text.setLowPriority
  const hasPendingAction = pendingAction !== null
  const isArchived = props.item.status === "archived"
  const isCompleted = props.item.status === "completed"
  const isSaved = props.item.status === "saved"
  const statusLabel = getInboxStatusLabel(props.item.status)
  const reasonLabel = getInboxReasonLabel(props.item.reason)

  async function performRowAction(label: string, action: () => Promise<unknown> | unknown) {
    if (pendingAction !== null) {
      return
    }

    setPendingAction(label)
    try {
      await action()
    } catch (error) {
      console.error(`Failed to run inbox row action: ${label}`, error)
      appToaster.create({
        description: text.theDaemonBackedInboxRowCouldNotBeUpdated,
        duration: 3200,
        title: `Couldn't ${label.toLowerCase()}`,
        type: "error",
      })
    } finally {
      setPendingAction(null)
    }
  }

  <div class={cx(styles.row, "group")}>
    <label class={styles.rowSelect}>
      <input
        aria-label={`Select ${getInboxItemPrimaryText(props.item)}`}
        checked={props.isSelected}
        class={styles.rowCheckbox}
        type="checkbox"
        onChange={(event) => {
          props.onSelectionChange?.(event.currentTarget.checked)
        }}
      />
    </label>
    <div
      ref={props.itemRef}
      class={styles.rowOpenButton}
      role="button"
      tabIndex={0}
      onClick={() => {
        mutations.openInboxItem(props.item)
      }}
      onKeyDown={(event) => {
        handleRowOpenKeyDown(event, () => mutations.openInboxItem(props.item))
      }}
    >
      <span
        aria-hidden="true"
        class={styles.rowUnreadMarker}
        data-visible={props.item.status === "unread" ? "true" : undefined}
      />
      <span class={styles.entityIcon} aria-hidden="true">
        <EntityIcon size={16} strokeWidth={2.1} />
      </span>
      <div class={styles.rowMain}>
        <div class={styles.rowMeta}>
          @if (props.showProjectName ?? true) {
            <span class={styles.entityLabel}>{getInboxEntityLabel(props.item.entityId)}</span>
          }

          <span class={styles.reasonLabel}>{reasonLabel}</span>
        </div>
        <div class={styles.rowCopy}>
          <h2 class={styles.rowTitle}>{getInboxItemPrimaryText(props.item)}</h2>
          <p class={styles.rowDescription}>{getInboxItemSecondaryText(props.item)}</p>
        </div>
      </div>
      <div class={styles.rowState}>
        <div class={styles.badgeRow}>
          <span class={styles.statusBadge} data-status={props.item.status}>{statusLabel}</span>
          <span
            class={styles.priorityBadge}
            data-priority={props.item.priority}
          >{priorityLabel}</span>
        </div>
        <time
          class={styles.updated}
          dateTime={new Date(props.item.updatedAt).toISOString()}
          title={new Intl.DateTimeFormat(undefined, {
            dateStyle: "medium",
            timeStyle: "short",
          }).format(new Date(props.item.updatedAt))}
        >{formatInboxUpdatedTime(props.item.updatedAt)}</time>
      </div>
    </div>
    <div
      class={styles.rowActions}
      aria-label={`${getInboxItemPrimaryText(props.item)} actions`}
      role="group"
    >
      <InboxRowActionButton
        ariaLabel={readLabel}
        disabled={hasPendingAction}
        icon={props.item.status === "unread" ? MailOpen : Mail}
        onPress={() => {
          void performRowAction(
            readLabel,
            () => mutations.updateInboxItem({
              entityId: props.item.entityId,
              status: readStatus,
            }),
          )
        }}
      />
      <InboxRowActionButton
        ariaLabel={isSaved ? text.saved : text.save}
        disabled={hasPendingAction || isSaved}
        icon={Bookmark}
        isActive={isSaved}
        onPress={() => {
          void performRowAction(
            text.save,
            () => mutations.updateInboxItem({
              entityId: props.item.entityId,
              status: "saved",
            }),
          )
        }}
      />
      <InboxRowActionButton
        ariaLabel={priorityActionLabel}
        disabled={hasPendingAction}
        icon={props.item.priority === "low" ? ArrowUp : ArrowDown}
        isActive={props.item.priority === "low"}
        onPress={() => {
          void performRowAction(
            priorityActionLabel,
            () => mutations.updateInboxItem({
              entityId: props.item.entityId,
              priority,
            }),
          )
        }}
      />

      @if (sessionItem) {
        <InboxRowActionButton
          ariaLabel={isCompleted ? text.completed : text.complete}
          disabled={hasPendingAction || isCompleted}
          icon={CheckCircle2}
          isActive={isCompleted}
          onPress={() => {
            void performRowAction(
              text.complete,
              () => mutations.completeSessionInboxItem({
                id: sessionItem.entityId,
              }),
            )
          }}
        />
      }

      <InboxRowActionButton
        ariaLabel={isArchived ? text.archived : text.archive}
        disabled={hasPendingAction || isArchived}
        icon={Archive}
        isActive={isArchived}
        onPress={() => {
          void performRowAction(
            text.archive,
            () => mutations.updateInboxItem({
              entityId: props.item.entityId,
              status: "archived",
            }),
          )
        }}
      />
    </div>
  </div>
}
