import { useMemo, useState } from "preact/hooks"
import type {
  CompleteSessionRequest,
  InboxItem,
  InboxStatus,
  UpdateInboxItemRequest,
} from "@goddard-ai/schema/daemon"

import { getInboxEntityKind, isInboxEntityKind } from "./entity-kind.ts"
import { DEFAULT_INBOX_FILTER_ID, inboxFilterDefinitions, type InboxFilterId } from "./filters.ts"
import { InboxList } from "./list.tsrx"
import { InboxPageMutations } from "./mutations.ts"
import { InboxToolbar } from "./toolbar.tsrx"
import {
  filterPreparedInboxItemsBySearch,
  parseInboxSearchQuery,
  prepareInboxSearchItems,
  replaceInboxSearchStatusFilters,
  type PreparedInboxSearchItem,
} from "./search.ts"
import { INBOX_DEBUG_ITEMS } from "./debug-data.ts"
import styles from "./debug-view.style.ts"

function filterDebugItemsByStatus(
  items: readonly PreparedInboxSearchItem[],
  filterId: InboxFilterId,
) {
  const statuses = new Set<InboxStatus>(inboxFilterDefinitions[filterId].statuses)
  return items.filter((entry) => statuses.has(entry.item.status))
}

function applyDebugInboxUpdate(item: InboxItem, input: UpdateInboxItemRequest, timestamp: number) {
  if (item.entityId !== input.entityId) {
    return item
  }

  const nextItem: InboxItem = {
    ...item,
    ...input.priority && { priority: input.priority },
    ...input.status &&
      {
        status: input.status,
        readAt: input.status === "read"
          ? timestamp
          : input.status === "unread"
            ? null
            : item.readAt,
      },
    updatedAt: timestamp,
  }
  return nextItem
}

function applyDebugSessionCompletion(
  item: InboxItem,
  input: CompleteSessionRequest,
  timestamp: number,
) {
  if (!isInboxEntityKind(item, "session") || item.entityId !== input.id) {
    return item
  }

  const nextItem: InboxItem = {
    ...item,
    status: "completed",
    updatedAt: timestamp,
  }
  return nextItem
}

export type InboxDebugViewProps = {
  items?: readonly InboxItem[];
}

export component InboxDebugView(props: InboxDebugViewProps) {
  const [items, setItems] = useState<readonly InboxItem[]>(props.items ?? INBOX_DEBUG_ITEMS)
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")
  const [selectedItem, setSelectedItem] = useState<InboxItem | null>(null)
  const parsedSearch = parseInboxSearchQuery(searchQuery)
  const preparedItems = useMemo(() => prepareInboxSearchItems(items), [items])
  const filteredItems =
    parsedSearch.isActive ? preparedItems : filterDebugItemsByStatus(preparedItems, filterId)
  const visibleItems = filterPreparedInboxItemsBySearch(filteredItems, parsedSearch)

  <InboxPageMutations
    mutations={{
      completeSessionInboxItem(input) {
        const timestamp = Date.now()
        setItems(
          (currentItems) => currentItems.map(
            (item) => applyDebugSessionCompletion(item, input, timestamp),
          ),
        )
        setSelectedItem((item) => item ? applyDebugSessionCompletion(item, input, timestamp) : null)
      },
      openInboxItem(item) {
        setSelectedItem(item)
      },
      updateInboxItem(input) {
        const timestamp = Date.now()
        setItems(
          (currentItems) => currentItems.map(
            (item) => applyDebugInboxUpdate(item, input, timestamp),
          ),
        )
        setSelectedItem((item) => item ? applyDebugInboxUpdate(item, input, timestamp) : null)
      },
    }}
  >
    <div class={styles.page}>
      <section class={styles.header}>
        <div class={styles.headingGroup}>
          <div class={styles.eyebrow}>{"Debug Surface"}</div>
          <h1 class={styles.title}>{"Inbox"}</h1>
          <p class={styles.body}>
            {"This surface renders deterministic inbox fixture rows through the real inbox toolbar, filter, fuzzy search, list, empty-state, and row-click code paths without loading daemon data."}
          </p>
        </div>
        <div class={styles.statRow}>
          <span class={styles.stat}>{`${items.length} fixture rows`}</span>
          <span class={styles.stat}>{`${filteredItems.length} after filter`}</span>
          <span class={styles.stat}>{`${visibleItems.length} visible`}</span>
        </div>
      </section>
      <section class={styles.canvas}>
        <div class={styles.inboxFrame}>
          <InboxToolbar
            {filterId}
            {searchQuery}
            onFilterSelect={(id) => {
              if (searchQuery.trim().length > 0) {
                setSearchQuery(
                  replaceInboxSearchStatusFilters(searchQuery, inboxFilterDefinitions[id].statuses),
                )
              } else {
                setFilterId(id)
              }
              setSelectedItem(null)
            }}
            onRefresh={() => {
              setItems(props.items ?? INBOX_DEBUG_ITEMS)
              setSearchQuery("")
              setSelectedItem(null)
            }}
            onSearchInput={(value) => {
              setSearchQuery(value)
              setSelectedItem(null)
            }}
          />
          <InboxList {filterId} items={visibleItems} {searchQuery} />
        </div>
        <div class={styles.selectedPanel}>
          <h2 class={styles.selectedTitle}>{"Selected fixture item"}</h2>
          <p class={styles.selectedBody}>
            {selectedItem
              ? `${selectedItem.id} · ${getInboxEntityKind(
                  selectedItem.entityId,
                )} · ${selectedItem.entityId} · ${selectedItem.status}`
              : "Click a row to verify row activation without opening daemon-backed detail tabs."}
          </p>
        </div>
      </section>
    </div>
  </InboxPageMutations>
}

export default InboxDebugView
