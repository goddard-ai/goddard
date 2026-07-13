import type { DaemonSession } from "@goddard-ai/sdk"
import { expect, test, vi } from "vitest"

import { applySessionControlUpdates } from "./apply-session-controls.ts"

function createSession(id: string) {
  return { id } as DaemonSession
}

test("session control application preserves order and syncs every returned state", async () => {
  const states = [createSession("mode-applied"), createSession("model-applied")]
  const apply = vi
    .fn()
    .mockResolvedValueOnce({ session: states[0] })
    .mockResolvedValueOnce({ session: states[1] })
  const sync = vi.fn()

  await applySessionControlUpdates({
    updates: [
      { id: "ses_session", configId: "mode", value: "full-auto" },
      { id: "ses_session", configId: "model", value: "opus" },
    ],
    apply,
    refresh: vi.fn(),
    sync,
  })

  expect(apply.mock.calls.map(([update]) => update.configId)).toEqual(["mode", "model"])
  expect(sync.mock.calls.map(([session]) => session.id)).toEqual(["mode-applied", "model-applied"])
})

test("session control application refreshes state and rejects after a partial failure", async () => {
  const failure = new Error("model rejected")
  const apply = vi
    .fn()
    .mockResolvedValueOnce({ session: createSession("mode-applied") })
    .mockRejectedValueOnce(failure)
  const sync = vi.fn()

  await expect(
    applySessionControlUpdates({
      updates: [
        { id: "ses_session", configId: "mode", value: "full-auto" },
        { id: "ses_session", configId: "model", value: "removed" },
      ],
      apply,
      refresh: vi.fn().mockResolvedValue(createSession("agent-state")),
      sync,
    }),
  ).rejects.toBe(failure)

  expect(sync.mock.calls.map(([session]) => session.id)).toEqual(["mode-applied", "agent-state"])
})
