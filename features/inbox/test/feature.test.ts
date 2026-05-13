import { describe, expect, test } from "bun:test"

import { inboxAppPlugin } from "../src/app.tsx"
import { inboxIpcSchema } from "../src/daemon-ipc.ts"
import { createInboxRequestHandlers, inboxPlugin } from "../src/daemon.ts"
import { InboxItemId } from "../src/schema.ts"
import { inboxSdkPlugin } from "../src/sdk.ts"

describe("inbox feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(inboxAppPlugin.name).toBe("inbox")
    expect(inboxAppPlugin.navigation).toMatchObject({ id: "inbox", slot: "primaryWorkbench" })
    expect(inboxPlugin.name).toBe("inbox")
    expect(Object.keys(inboxIpcSchema.requests)).toEqual([
      "inbox.list",
      "inbox.update",
      "inbox.bulkUpdate",
    ])
    expect(inboxSdkPlugin.namespace).toBe("inbox")
    expect(InboxItemId.parse("inb_test")).toBe("inb_test")
  })

  test("creates daemon request handlers from injected inbox manager", async () => {
    const handlers = createInboxRequestHandlers({
      inboxManager: {
        listInboxItems: () => ({ items: [], nextCursor: null, hasMore: false }),
        updateInboxItem: () => {
          throw new Error("unexpected update")
        },
        bulkUpdateInboxItems: () => {
          throw new Error("unexpected bulk update")
        },
      },
    })

    await expect(handlers["inbox.list"]({})).resolves.toEqual({
      items: [],
      nextCursor: null,
      hasMore: false,
    })
  })
})
