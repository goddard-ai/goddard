import { beforeEach, describe, expect, test, vi } from "vitest"

const { updateMock } = vi.hoisted(() => ({
  updateMock: vi.fn(async () => undefined),
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    update: updateMock,
  },
}))

import { declareInitiative, reportBlocker, reportCompleted } from "../src/bin/goddard-tool.js"

describe("tool session updates", () => {
  beforeEach(() => {
    updateMock.mockClear()
  })

  test("declare initiative stores latest initiative", async () => {
    await declareInitiative("session-1", "Ship websocket cancellation")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      initiative: "Ship websocket cancellation",
      blockedReason: null,
      status: "active",
    })
  })

  test("report blocker stores reason and blocked status", async () => {
    await reportBlocker("session-1", "Needs maintainer review")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      status: "blocked",
      blockedReason: "Needs maintainer review",
    })
  })

  test("report completed clears initiative and returns session to done", async () => {
    await reportCompleted("session-1")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      initiative: null,
      blockedReason: null,
      status: "done",
    })
  })
})
