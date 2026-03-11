import { describe, expect, test, vi } from "vitest"

const { sessionStorageGet } = vi.hoisted(() => ({
  sessionStorageGet: vi.fn(async () => ({ serverPid: 4242 })),
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    get: sessionStorageGet,
  },
}))

import { AgentSession } from "../src/client-session.js"

describe("AgentSession.stop", () => {
  test("SIGTERMs stored session server PID when no local subprocess handle exists", async () => {
    const close = vi.fn()
    const fetchMock = vi.fn(async () => {
      throw new Error("offline")
    })
    vi.stubGlobal("fetch", fetchMock)

    const processKillSpy = vi.spyOn(process, "kill").mockImplementation(() => true)

    const session = new AgentSession(
      "session-1",
      {
        prompt: vi.fn(),
        cancel: vi.fn(),
      } as any,
      "http://localhost:3001",
      { close },
      undefined,
    )

    await session.stop()

    expect(close).toHaveBeenCalledOnce()
    expect(sessionStorageGet).toHaveBeenCalledWith("session-1")
    expect(processKillSpy).toHaveBeenCalledWith(4242, "SIGTERM")

    processKillSpy.mockRestore()
    vi.unstubAllGlobals()
  })
})
