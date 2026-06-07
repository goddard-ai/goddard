import type { InboxEntityId, InboxItem } from "@goddard-ai/inbox/schema"

import { inboxFilterDefinitions, type InboxFilterId } from "./filters.ts"
import { InboxRow } from "./row.tsrx"
import styles from "./page.style.ts"

export component InboxList(props: {
  activeItemId?: InboxItem["id"] | null;
  errorMessage?: string | null;
  filterId: InboxFilterId;
  items: readonly InboxItem[];
  listStatus?: "idle" | "loading" | "ready" | "error";
  selectedEntityIds?: ReadonlySet<InboxEntityId>;
  searchQuery: string;
  onItemSelectionChange?: (item: InboxItem, isSelected: boolean) => void;
}) {
  const filter = inboxFilterDefinitions[props.filterId]
  const hasSearchQuery = props.searchQuery.trim().length > 0

  if (props.listStatus === "loading" && props.items.length === 0) {
    <div class={styles.loading}>{"Loading inbox..."}</div>

    return
  }

  if (props.listStatus === "error" && props.items.length === 0) {
    <div class={styles.error}>
      <div class={styles.messageContent}>
        <h2 class={styles.messageTitle}>{"Couldn't load inbox"}</h2>
        <p class={styles.messageBody}>
          {props.errorMessage ?? "The daemon-backed inbox request failed."}
        </p>
      </div>
    </div>

    return
  }

  if (props.items.length === 0) {
    <div class={styles.empty}>
      <div class={styles.messageContent}>
        <h2 class={styles.messageTitle}>
          {hasSearchQuery ? "No matching inbox items" : filter.emptyTitle}
        </h2>
        <p class={styles.messageBody}>
          {hasSearchQuery
            ? "Try a shorter search or a status filter like is:unread."
            : filter.emptyDescription}
        </p>
      </div>
    </div>

    return
  }

  <ul class={styles.list} aria-label={`${filter.label} inbox items`}>
    for (const item of props.items) {
      const onSelectionChange =
        props.onItemSelectionChange
          ? (isSelected: boolean) => {
              props.onItemSelectionChange?.(item, isSelected)
            }
          : undefined

      <li key={item.id}>
        <InboxRow
          isActive={props.activeItemId === item.id}
          {item}
          isSelected={props.selectedEntityIds?.has(item.entityId) ?? false}
          {onSelectionChange}
        />
      </li>
    }
  </ul>
}
