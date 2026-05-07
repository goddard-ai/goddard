import { useState } from "preact/hooks"

import { goddardSdk } from "~/sdk.ts"
import { useQuery } from "~/lib/query.ts"
import { InboxList } from "./list.tsrx"
import { invalidateInboxQueries } from "./mutations.ts"
import { InboxToolbar } from "./toolbar.tsrx"
import { getInboxListRequest } from "./queries.ts"
import {
  DEFAULT_INBOX_FILTER_ID,
  filterInboxItemsBySearch,
  type InboxFilterId,
} from "./presentation.ts"
import styles from "./page.style.ts"

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon-backed inbox request failed."
}

component InboxQueryContent(props: {
  filterId: InboxFilterId;
  searchQuery: string;
}) {
  const { items } = useQuery(goddardSdk.inbox.list, [getInboxListRequest(props.filterId)])
  const visibleItems = filterInboxItemsBySearch(items, props.searchQuery)

  <InboxList filterId={props.filterId} items={visibleItems} searchQuery={props.searchQuery} />
}

component InboxPage() {
  const [filterId, setFilterId] = useState<InboxFilterId>(DEFAULT_INBOX_FILTER_ID)
  const [searchQuery, setSearchQuery] = useState("")

  <div class={styles.root}>
    <InboxToolbar
      {filterId}
      {searchQuery}
      onFilterSelect={(id) => {
        setFilterId(id)
      }}
      onRefresh={() => {
        invalidateInboxQueries()
      }}
      onSearchInput={(value) => {
        setSearchQuery(value)
      }}
    />
    <div class={styles.listBody}>
      try {
        <InboxQueryContent {filterId} {searchQuery} />
      } pending {
        <InboxList {filterId} items={[]} listStatus="loading" {searchQuery} />
      } catch (error) {
        <InboxList
          errorMessage={getErrorMessage(error)}
          {filterId}
          items={[]}
          listStatus="error"
          {searchQuery}
        />
      }
    </div>
  </div>
}

export default InboxPage
