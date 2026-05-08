import type { InboxItem } from "@goddard-ai/schema/daemon"
import * as fuzzysort from "fuzzysort2"

import { getInboxItemSearchText } from "./text.ts"

function normalizeSearchText(value: string) {
  return value.trim().toLowerCase()
}

/** Filters inbox rows by fuzzy matching human-visible row text. */
export function filterInboxItemsBySearch(items: readonly InboxItem[], searchQuery: string) {
  const normalizedQuery = normalizeSearchText(searchQuery)

  if (normalizedQuery.length === 0) {
    return items
  }

  const searchableItems = items.map((item) => ({
    item,
    preparedSearchText: fuzzysort.prepare(normalizeSearchText(getInboxItemSearchText(item))),
  }))

  return fuzzysort
    .searchFields(
      normalizedQuery,
      searchableItems,
      [{ key: "searchText", extract: (entry) => entry.preparedSearchText }],
      { threshold: 0 },
    )
    .items.map((entry) => entry.value.item)
}
