import { describe, expect, test } from "bun:test"

import { inboxIpcRoutes } from "../src/daemon-ipc.ts"
import { inboxPlugin } from "../src/daemon.ts"
import { InboxItemId } from "../src/schema.ts"
import { inboxSdkPlugin } from "../src/sdk.ts"

describe("inbox feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(inboxPlugin.name).toBe("inbox")
    expect(Object.keys(inboxIpcRoutes.inbox.children)).toEqual([
      "list",
      "update",
      "bulkUpdate",
      "completeSession",
    ])
    expect(inboxSdkPlugin.name).toBe("inbox")
    expect(InboxItemId.parse("inb_test")).toBe("inb_test")
  })
})
