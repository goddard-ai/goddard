import type {
  BulkUpdateInboxItemsRequest,
  CompleteSessionRequest,
  InboxItem,
  UpdateInboxItemRequest,
} from "@goddard-ai/schema/daemon"

import { createMutationsProvider } from "~/lib/mutations-provider.tsx"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

export const InboxPageMutations = createMutationsProvider<{
  bulkUpdateInboxItems: (input: BulkUpdateInboxItemsRequest) => Promise<unknown> | unknown
  completeSessionInboxItem: (input: CompleteSessionRequest) => Promise<unknown> | unknown
  openInboxItem: (item: InboxItem) => void
  updateInboxItem: (input: UpdateInboxItemRequest) => Promise<unknown> | unknown
}>("InboxPageMutations")

/** Invalidates every cached daemon inbox list, regardless of filter or cursor. */
export function invalidateInboxQueries() {
  queryClient.invalidate(goddardSdk.inbox.list)
}

/** Updates one daemon inbox row and refreshes any mounted inbox lists. */
export async function updateInboxItem(input: UpdateInboxItemRequest) {
  const result = await goddardSdk.inbox.update(input)
  invalidateInboxQueries()
  return result
}

/** Updates multiple daemon inbox rows and refreshes any mounted inbox lists. */
export async function bulkUpdateInboxItems(input: BulkUpdateInboxItemsRequest) {
  const result = await goddardSdk.inbox.bulkUpdate(input)
  invalidateInboxQueries()
  return result
}

/** Completes one session-owned inbox row through the entity-specific daemon mutation. */
export async function completeSessionInboxItem(input: CompleteSessionRequest) {
  const result = await goddardSdk.session.complete(input)
  invalidateInboxQueries()
  return result
}
