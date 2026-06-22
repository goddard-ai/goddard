import { event } from "@goddard-ai/sdk-plugin"

import type { InboxItem } from "./schema.ts"

export const inboxEvents = {
  "inbox.item.updated": event<InboxItem>({ debug: "inbox.stream" }),
}

export type InboxEventDefinitions = typeof inboxEvents
