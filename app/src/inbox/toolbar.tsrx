import { t } from "@lingui/core/macro"
import { Search } from "lucide-react"

import { inboxFilterDefinitions, inboxFilterOrder, type InboxFilterId } from "./filters.ts"
import { getInboxSearchActiveFilterIds } from "./search.ts"
import styles from "./page.style.ts"

export function InboxToolbar(props: {
  filterId: InboxFilterId;
  inputRef?: (element: HTMLInputElement | null) => void | (() => void);
  searchQuery: string;
  onFilterSelect: (id: InboxFilterId) => void;
  onSearchInput?: (value: string) => void;
}) @{
  const hasSearchQuery = props.searchQuery.trim().length > 0
  const activeSearchFilterIds = new Set(getInboxSearchActiveFilterIds(props.searchQuery))

  <div class={styles.toolbar}>
    <div class={styles.headingGroup}>
      <h1 class={styles.heading}>
        {t`Inbox`}
      </h1>
      <div class={styles.filterGroup} aria-label={t`Inbox filters`}>
        @for (const id of inboxFilterOrder) {
          const filter = inboxFilterDefinitions[id]
          const isSelected =
            hasSearchQuery ? activeSearchFilterIds.has(id) : props.filterId === id

          <button
            key={id}
            aria-pressed={isSelected}
            class={styles.filterButton}
            data-selected={isSelected ? "true" : undefined}
            type="button"
            onClick={() => {
              props.onFilterSelect(id)
            }}
          >{filter.label}</button>
        }
      </div>
    </div>
    <div class={styles.toolbarControls}>
      <label class={styles.searchField}>
        <span class={styles.srOnly}>
          {t`Search inbox items`}
        </span>
        <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
        <input
          ref={props.inputRef}
          class={styles.searchInput}
          placeholder={t`Search inbox`}
          type="search"
          value={props.searchQuery}
          onInput={(event) => {
            props.onSearchInput?.(event.currentTarget.value)
          }}
        />
      </label>
    </div>
  </div>
}
