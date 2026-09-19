import { expect, test } from "bun:test";

import type { AgentSession } from "./generated";
import { sessionProviderLocked } from "./session-state";

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
