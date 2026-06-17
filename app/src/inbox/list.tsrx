import type { InboxEntityId, InboxItem } from "@goddard-ai/inbox/schema"

import { text } from "~/language/text.ts"
import { inboxFilterDefinitions, type InboxFilterId } from "./filters.ts"
import { InboxRow } from "./row.tsrx"
import styles from "./page.style.ts"

export function InboxList(props: {
  errorMessage?: string | null;
  filterId: InboxFilterId;
  itemRef?: (index: number) => (element: HTMLElement | null) => void | (() => void);
  items: readonly InboxItem[];
  listStatus?: "idle" | "loading" | "ready" | "error";
  selectedEntityIds?: ReadonlySet<InboxEntityId>;
  searchQuery: string;
  showProjectNames?: boolean;
  onItemSelectionChange?: (item: InboxItem, isSelected: boolean) => void;
}) @{
  const filter = inboxFilterDefinitions[props.filterId]
  const hasSearchQuery = props.searchQuery.trim().length > 0

  if (props.listStatus === "loading" && props.items.length === 0) {
    return <div class={styles.loading}>{text.loadingInbox}</div>
  }

  if (props.listStatus === "error" && props.items.length === 0) {
    return <div class={styles.error}>
      <div class={styles.messageContent}>
        <h2 class={styles.messageTitle}>{text.couldntLoadInbox}</h2>
        <p class={styles.messageBody}>
          {props.errorMessage ?? text.theDaemonBackedInboxRequestFailed}
        </p>
      </div>
    </div>
  }

  if (props.items.length === 0) {
    return <div class={styles.empty}>
      <div class={styles.messageContent}>
        <h2 class={styles.messageTitle}>
          {hasSearchQuery ? text.noMatchingInboxItems : filter.emptyTitle}
        </h2>
        <p class={styles.messageBody}>
          {hasSearchQuery
            ? text.tryAShorterSearchOrAStatusFilterLikeIsUnread
            : filter.emptyDescription}
        </p>
      </div>
    </div>
  }

  <ul class={styles.list} aria-label={`${filter.label} ${text.inboxItems}`}>
    @for (const item of props.items; index index) {
      const onSelectionChange =
        props.onItemSelectionChange
          ? (isSelected: boolean) => {
              props.onItemSelectionChange?.(item, isSelected)
            }
          : undefined

      <li key={item.id}>
        <InboxRow
          itemRef={props.itemRef?.(index)}
          {item}
          isSelected={props.selectedEntityIds?.has(item.entityId) ?? false}
          showProjectName={props.showProjectNames}
          {onSelectionChange}
        />
      </li>
    }
  </ul>
}
