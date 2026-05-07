import type {
  BulkUpdateInboxItemsRequest,
  InboxItem,
  UpdateInboxItemRequest,
} from "@goddard-ai/schema/daemon"

import { createMutationsProvider } from "~/lib/mutations-provider.tsx"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

export const InboxPageMutations = createMutationsProvider<{
  openInboxItem: (item: InboxItem) => void
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
