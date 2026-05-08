import type { InboxItem, InboxStatus } from "@goddard-ai/schema/daemon"
import * as fuzzysort from "fuzzysort2"

import { getInboxEntityLabel } from "./entity-kind.ts"
import { allInboxStatuses, inboxFilterDefinitions, inboxFilterOrder } from "./filters.ts"
import { getInboxPriorityLabel, getInboxReasonLabel, getInboxStatusLabel } from "./labels.ts"
import { getInboxItemPrimaryText, getInboxItemSecondaryText } from "./text.ts"

/** Cached fuzzy-search targets for one inbox row. */
export type PreparedInboxSearchItem = {
  item: InboxItem
  preparedEntityId: fuzzysort.PreparedTarget
  preparedEntityLabel: fuzzysort.PreparedTarget
  preparedPrimaryText: fuzzysort.PreparedTarget
  preparedPriorityLabel: fuzzysort.PreparedTarget
  preparedReasonLabel: fuzzysort.PreparedTarget
  preparedSecondaryText: fuzzysort.PreparedTarget
  preparedStatusLabel: fuzzysort.PreparedTarget
}

/** Parsed inbox search input, including optional status filters. */
export type InboxSearchQuery = {
  fuzzyQuery: string
  isActive: boolean
  statuses: readonly InboxStatus[] | null
}

const statusFilterPrefixes = ["is:", "status:"] as const

function normalizeSearchQuery(value: string) {
  return value.trim()
}

function hasStatusFilterPrefix(token: string) {
  const normalizedToken = token.toLowerCase()
  return statusFilterPrefixes.some((candidate) => normalizedToken.startsWith(candidate))
}

function isInboxStatus(value: string): value is InboxStatus {
  return (allInboxStatuses as readonly string[]).includes(value)
}

function parseStatusFilterToken(token: string) {
  const normalizedToken = token.toLowerCase()
  const prefix = statusFilterPrefixes.find((candidate) => normalizedToken.startsWith(candidate))

  if (!prefix) {
    return null
  }

  const status = normalizedToken.slice(prefix.length)
  return isInboxStatus(status) ? status : null
}

/** Parses status filters such as `is:unread` out of one inbox search query. */
export function parseInboxSearchQuery(searchQuery: string) {
  const normalizedQuery = normalizeSearchQuery(searchQuery)
  const fuzzyTokens: string[] = []
  const statusSet = new Set<InboxStatus>()

  for (const token of normalizedQuery.split(/\s+/)) {
    const status = parseStatusFilterToken(token)

    if (status) {
      statusSet.add(status)
    } else if (token.length > 0) {
      fuzzyTokens.push(token)
    }
  }

  const statuses = allInboxStatuses.filter((status) => statusSet.has(status))

  return {
    fuzzyQuery: fuzzyTokens.join(" "),
    isActive: normalizedQuery.length > 0,
    statuses: statuses.length > 0 ? statuses : null,
  } satisfies InboxSearchQuery
}

/** Replaces inline status filters with the given statuses while preserving fuzzy query terms. */
export function replaceInboxSearchStatusFilters(
  searchQuery: string,
  statuses: readonly InboxStatus[],
) {
  const remainingTokens = normalizeSearchQuery(searchQuery)
    .split(/\s+/)
    .filter((token) => token.length > 0 && !hasStatusFilterPrefix(token))
  const statusTokens = statuses.map((status) => `is:${status}`)

  return [...statusTokens, ...remainingTokens].join(" ")
}

/** Returns the inbox filter buttons represented by parsed inline status filters. */
export function getInboxSearchActiveFilterIds(searchQuery: string) {
  const parsedSearch = parseInboxSearchQuery(searchQuery)

  if (!parsedSearch.statuses) {
    return []
  }

  return inboxFilterOrder.filter((filterId) => {
    const filterStatuses = inboxFilterDefinitions[filterId].statuses
    return filterStatuses.some((status) => parsedSearch.statuses?.includes(status))
  })
}

/** Prepares reusable fuzzy-search targets for inbox rows. */
export function prepareInboxSearchItems(items: readonly InboxItem[]) {
  return items.map((item) => ({
    item,
    preparedEntityId: fuzzysort.prepare(item.entityId),
    preparedEntityLabel: fuzzysort.prepare(getInboxEntityLabel(item.entityId)),
    preparedPrimaryText: fuzzysort.prepare(getInboxItemPrimaryText(item)),
    preparedPriorityLabel: fuzzysort.prepare(getInboxPriorityLabel(item.priority)),
    preparedReasonLabel: fuzzysort.prepare(getInboxReasonLabel(item.reason)),
    preparedSecondaryText: fuzzysort.prepare(getInboxItemSecondaryText(item)),
    preparedStatusLabel: fuzzysort.prepare(getInboxStatusLabel(item.status)),
  }))
}

/** Filters prepared inbox rows by fuzzy matching human-visible row fields. */
export function filterPreparedInboxItemsBySearch(
  items: readonly PreparedInboxSearchItem[],
  searchQuery: string | InboxSearchQuery,
) {
  const parsedSearch =
    typeof searchQuery === "string" ? parseInboxSearchQuery(searchQuery) : searchQuery
  const statusFilteredItems = parsedSearch.statuses
    ? items.filter((entry) => parsedSearch.statuses?.includes(entry.item.status))
    : items

  if (parsedSearch.fuzzyQuery.length === 0) {
    return statusFilteredItems.map((entry) => entry.item)
  }

  return fuzzysort
    .searchFields(
      parsedSearch.fuzzyQuery,
      statusFilteredItems,
      [
        { key: "entityLabel", extract: (entry) => entry.preparedEntityLabel },
        { key: "statusLabel", extract: (entry) => entry.preparedStatusLabel },
        { key: "priorityLabel", extract: (entry) => entry.preparedPriorityLabel },
        { key: "reasonLabel", extract: (entry) => entry.preparedReasonLabel },
        { key: "primaryText", extract: (entry) => entry.preparedPrimaryText },
        { key: "secondaryText", extract: (entry) => entry.preparedSecondaryText },
        { key: "entityId", extract: (entry) => entry.preparedEntityId },
      ],
      { threshold: 0 },
    )
    .items.map((entry) => entry.value.item)
}
