import { useState } from "preact/hooks"
import type { InboxItem, InboxStatus } from "@goddard-ai/schema/daemon"

import { getInboxEntityKind } from "./entity-kind.ts"
import { DEFAULT_INBOX_FILTER_ID, inboxFilterDefinitions, type InboxFilterId } from "./filters.ts"
import { InboxList } from "./list.tsrx"
import { InboxPageMutations } from "./mutations.ts"
import { InboxToolbar } from "./toolbar.tsrx"
import { filterInboxItemsBySearch } from "./search.ts"
import { INBOX_DEBUG_ITEMS } from "./debug-data.ts"
import styles from "./debug-view.style.ts"

function filterDebugItemsByStatus(items: readonly InboxItem[], filterId: InboxFilterId) {
  const statuses = new Set<InboxStatus>(inboxFilterDefinitions[filterId].statuses)
  return items.filter((item) => statuses.has(item.status))
}

export type InboxDebugViewProps = {
  items?: readonly InboxItem[];
}

export component InboxDebugView(props: InboxDebugViewProps) {
  const items = props.items ?? INBOX_DEBUG_ITEMS
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")
  const [selectedItem, setSelectedItem] = useState<InboxItem | null>(null)
  const filteredItems = filterDebugItemsByStatus(items, filterId)
  const visibleItems = filterInboxItemsBySearch(filteredItems, searchQuery)

  <InboxPageMutations
    mutations={{
      openInboxItem(item) {
        setSelectedItem(item)
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
              setFilterId(id)
              setSelectedItem(null)
            }}
            onRefresh={() => {
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
