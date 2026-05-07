import { expect, test } from "bun:test"

import {
  createSessionChatQaChatWithAllHistory,
  createSessionChatQaStatusSummary,
} from "./transcript-debug-data.ts"

test("session chat QA scenario covers every generated transcript row family", () => {
  const messages = createSessionChatQaChatWithAllHistory().transcriptMessages
  const rowKinds = new Set(messages.map((message) => message.kind))
  const textRoles = new Set(
    messages.flatMap((message) => (message.kind === "message" ? [message.role] : [])),
  )
  const toolStatuses = new Set(
    messages.flatMap((message) => (message.kind === "toolCall" ? [message.status] : [])),
  )
  const permissionStatuses = new Set(
    messages.flatMap((message) => (message.kind === "permissionRequest" ? [message.status] : [])),
  )
  const turnStopStatuses = new Set(
    messages.flatMap((message) => (message.kind === "turnStop" ? [message.status] : [])),
  )

  expect(rowKinds).toEqual(
    new Set(["message", "toolCall", "permissionRequest", "planUpdate", "turnStop"]),
  )
  expect(textRoles).toEqual(new Set(["system", "user", "assistant"]))
  expect(toolStatuses).toEqual(new Set(["pending", "in_progress", "completed", "failed"]))
  expect(permissionStatuses).toEqual(
    new Set(["pending", "allowed", "denied", "failed", "cancelled", "resolved"]),
  )
  expect(turnStopStatuses).toEqual(
    new Set(["completed", "stopped", "failed", "cancelled", "interrupted"]),
  )
  expect(
    messages.some(
      (message) =>
        message.kind === "message" &&
        message.content.some((block) => block.type === "resource_link"),
    ),
  ).toBe(true)
})

test("session chat QA status matrix covers header-relevant session states", () => {
  const statusSummary = createSessionChatQaStatusSummary()

  expect(new Set(statusSummary.map((status) => status.status))).toEqual(
    new Set(["idle", "running", "blocked", "completed", "failed", "cancelled"]),
  )
  expect(statusSummary.some((status) => status.actions.includes("stop"))).toBe(true)
  expect(statusSummary.some((status) => status.actions.includes("reconnect"))).toBe(true)
  expect(statusSummary.every((status) => status.actions.includes("changes"))).toBe(true)
})
