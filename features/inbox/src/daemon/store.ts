import type { DbContext } from "@goddard-ai/daemon-plugin"
import { kind } from "kindstore"

import { InboxItem } from "../schema.ts"

/** Kindstore schema fragment owned by the inbox daemon feature. */
export const inboxDbSchema = {
  inboxItems: kind("inb", InboxItem.omit({ id: true }))
    .index("entityId", { type: "text", unique: true })
    .index("status")
    .multi("updatedAt_id", {
      updatedAt: "desc",
      id: "desc",
    }),
}

/** Scoped store surface required by the inbox manager. */
export type InboxStore = DbContext<typeof inboxDbSchema>
