import { event, type EventDefinition } from "@goddard-ai/sdk-plugin"

import type { InboxItem } from "./schema.ts"

export type InboxEventDefinitions = {
  "inbox.item.updated": EventDefinition<InboxItem>
}

export const inboxEvents = {
  "inbox.item.updated": event<InboxItem>({ debug: "inbox.stream" }),
} satisfies InboxEventDefinitions
