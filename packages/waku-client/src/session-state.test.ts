import { expect, test } from "bun:test";

import type { AgentSession, QueuedMessage } from "./generated";
import {
  appendQueuedSteerPreview,
  dropQueuedMessage,
  queuedMessageIsPendingSteer,
  sessionAcceptsImmediateSteer,
  sessionProviderLocked,
} from "./session-state";

function session(overrides: Partial<AgentSession>): AgentSession {
  return {
    detail_loaded: true,
    messages: [],
    provider_cursor: null,
    provider_session_id: null,
    turns: [],
    ...overrides,
  } as AgentSession;
}

test("assistant-only receipts do not lock the session provider", () => {
  const transfer = session({
    messages: [{ role: "assistant" } as never],
    turns: [{ provider_turn_started: false } as never],
  });

  expect(sessionProviderLocked(transfer)).toBe(false);
});

test("provider conversations lock the session provider", () => {
  expect(sessionProviderLocked(session({
    messages: [{ role: "user" } as never],
  }))).toBe(true);
  expect(sessionProviderLocked(session({
    turns: [{ provider_turn_started: true } as never],
  }))).toBe(true);
  expect(sessionProviderLocked(session({ detail_loaded: false }))).toBe(true);
});

test("steering reaches thinking and tools, but queues during assistant text", () => {
  const running = session({
    status: "working",
    turns: [{ status: "running", provider_turn_started: true } as never],
  });
  expect(sessionAcceptsImmediateSteer(running)).toBe(true);
  running.messages = [{ role: "assistant", streaming: true } as never];
  expect(sessionAcceptsImmediateSteer(running)).toBe(false);
  running.messages[0]!.streaming = false;
  expect(sessionAcceptsImmediateSteer(running)).toBe(true);
  running.status = "waiting";
  expect(sessionAcceptsImmediateSteer(running)).toBe(true);
  running.status = "background";
  expect(sessionAcceptsImmediateSteer(running)).toBe(true);
  running.status = "connecting";
  expect(sessionAcceptsImmediateSteer(running)).toBe(false);
  running.status = "working";
  running.turns[0]!.provider_turn_started = false;
  expect(sessionAcceptsImmediateSteer(running)).toBe(false);
});

test("a parked steer preview shares the queue entry's id until it drains", () => {
  const queued = { id: "queued-1", content: "steer here", created_at: 7 } as QueuedMessage;
  const base = session({
    messages: [{ id: "prompt", role: "user" } as never],
    queued_messages: [queued],
  });

  expect(queuedMessageIsPendingSteer(base, queued)).toBe(false);

  const parked = appendQueuedSteerPreview(base, queued);
  const preview = parked.messages.at(-1)!;
  expect(preview).toMatchObject({
    id: "queued-1",
    turn_id: null,
    role: "user",
    content: "steer here",
  });
  expect(queuedMessageIsPendingSteer(parked, queued)).toBe(true);
  // Idempotent — a second append can't duplicate the row.
  expect(appendQueuedSteerPreview(parked, queued)).toBe(parked);
  // A hidden entry stays provider-facing: no preview.
  expect(appendQueuedSteerPreview(base, { ...queued, hidden: true })).toBe(base);
});

test("dropping a queued message clears its preview row too", () => {
  const other = { id: "queued-2", content: "follow up", created_at: 8 } as QueuedMessage;
  const parked = appendQueuedSteerPreview(session({
    messages: [
      { id: "prompt", role: "user" } as never,
      { id: "answer", role: "assistant" } as never,
    ],
  }), { id: "queued-1", content: "steer here", created_at: 7 } as QueuedMessage);
  const queued = { ...parked, queued_messages: [
    { id: "queued-1", content: "steer here", created_at: 7 } as QueuedMessage,
    other,
  ] };

  const dropped = dropQueuedMessage(queued, "queued-1");
  expect(dropped.messages.map((message) => message.id)).toEqual(["prompt", "answer"]);
  expect(dropped.queued_messages).toEqual([other]);
});
