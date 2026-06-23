import { expect, test } from "bun:test"

import { matchesEventEnvelopeFilter } from "../src/index.ts"

test("event envelope filters match names and exact payload paths", () => {
  const envelope = {
    name: "session.activated",
    payload: {
      sessionId: "ses_123",
      worktree: {
        state: "mounted",
        counters: [1, { done: true }],
      },
    },
  }

  expect(
    matchesEventEnvelopeFilter(envelope, {
      names: ["session.activated"],
      where: [
        { path: "sessionId", equals: "ses_123" },
        { path: "worktree.state", equals: "mounted" },
        { path: "worktree.counters", equals: [1, { done: true }] },
      ],
    }),
  ).toBe(true)
  expect(
    matchesEventEnvelopeFilter(envelope, {
      names: ["session.launch.failed"],
    }),
  ).toBe(false)
  expect(
    matchesEventEnvelopeFilter(envelope, {
      where: [{ path: "worktree.missing", equals: "mounted" }],
    }),
  ).toBe(false)
  expect(
    matchesEventEnvelopeFilter(envelope, {
      where: [{ path: "worktree.state", equals: "prepared" }],
    }),
  ).toBe(false)
})
