import { describe, expect, test } from "bun:test"

import { inboxAppPlugin } from "../src/app.tsx"
import { inboxIpcRoutes } from "../src/daemon-ipc.ts"
import { inboxPlugin } from "../src/daemon.ts"
import { InboxItemId } from "../src/schema.ts"
import { inboxSdkPlugin } from "../src/sdk.ts"

describe("inbox feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(inboxAppPlugin.name).toBe("inbox")
    expect(inboxAppPlugin.navigation).toMatchObject({ id: "inbox", slot: "primaryWorkbench" })
    expect(inboxPlugin.name).toBe("inbox")
    expect(Object.keys(inboxIpcRoutes.inbox.children)).toEqual([
      "list",
      "update",
      "bulkUpdate",
      "completeSession",
      "streamItems",
    ])
    expect(inboxSdkPlugin.name).toBe("inbox")
    expect(InboxItemId.parse("inb_test")).toBe("inb_test")
  })
})
