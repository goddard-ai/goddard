import { beforeEach, describe, expect, test, vi } from "vitest"

const { createDaemonIpcClientFromEnvMock, unsubscribeMock, sendMock, subscribeMock } =
  vi.hoisted(() => {
    const sendMock = vi.fn()
    const unsubscribeMock = vi.fn()
    const subscribeMock = vi.fn(async () => unsubscribeMock)
    const clientMock = {
      send: sendMock,
      subscribe: subscribeMock,
    }

    return {
      sendMock,
      subscribeMock,
      unsubscribeMock,
      clientMock,
      createDaemonIpcClientFromEnvMock: vi.fn(() => ({ client: clientMock })),
    }
  })

vi.mock("@goddard-ai/daemon-client", () => ({
  createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
}))

vi.mock("@agentclientprotocol/sdk", () => ({
  AGENT_METHODS: {
    session_prompt: "session/prompt",
    session_cancel: "session/cancel",
  },
  ndJsonStream: vi.fn(() => ({
    readable: new ReadableStream(),
    writable: new WritableStream(),
  })),
  ClientSideConnection: class MockClientSideConnection {
    prompt = vi.fn()
    cancel = vi.fn()
  },
}))

describe("runAgent", () => {
  beforeEach(() => {
    sendMock.mockReset()
    subscribeMock.mockClear()
    unsubscribeMock.mockClear()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  test("creates one-shot daemon sessions and returns null", async () => {
    sendMock.mockResolvedValueOnce({
      session: {
        id: "daemon-session-1",
        acpId: "acp-session-1",
      },
    })

    const { runAgent } = await import("../../src/daemon/session/client.js")

    await expect(
      runAgent({
        agent: "pi",
        cwd: "/tmp/project",
        mcpServers: [],
        systemPrompt: "Follow the spec.",
        initialPrompt: "Ship it",
        oneShot: true,
      }),
    ).resolves.toBeNull()

    expect(sendMock).toHaveBeenCalledWith("sessionCreate", {
      agent: "pi",
      cwd: "/tmp/project",
      mcpServers: [],
      systemPrompt: "Follow the spec.",
      env: undefined,
      metadata: undefined,
      initialPrompt: "Ship it",
      oneShot: true,
    })
    expect(subscribeMock).not.toHaveBeenCalled()
  })

  test("connects to daemon-hosted sessions and delegates history/shutdown over IPC", async () => {
    sendMock
      .mockResolvedValueOnce({
        session: {
          id: "daemon-session-2",
          acpId: "acp-session-2",
        },
      })
      .mockResolvedValueOnce({
        id: "daemon-session-2",
        acpId: "acp-session-2",
        history: [{ jsonrpc: "2.0", method: "session/update", params: {} }],
      })
      .mockResolvedValueOnce({
        id: "daemon-session-2",
        success: true,
      })

    const { runAgent } = await import("../../src/daemon/session/client.js")

    const session = await runAgent({
      sessionId: "daemon-session-2",
      agent: "pi",
      cwd: "/tmp/project",
      mcpServers: [],
      systemPrompt: "Follow the spec.",
    })

    expect(session?.sessionId).toBe("daemon-session-2")
    await expect(session?.getHistory()).resolves.toEqual([
      { jsonrpc: "2.0", method: "session/update", params: {} },
    ])

    await session?.stop()

    expect(sendMock).toHaveBeenNthCalledWith(1, "sessionConnect", { id: "daemon-session-2" })
    expect(sendMock).toHaveBeenNthCalledWith(2, "sessionHistory", { id: "daemon-session-2" })
    expect(sendMock).toHaveBeenNthCalledWith(3, "sessionShutdown", { id: "daemon-session-2" })
    expect(subscribeMock).toHaveBeenCalledTimes(1)
    expect(unsubscribeMock).toHaveBeenCalledTimes(1)
  })

  test("uses an explicitly injected daemon client when provided", async () => {
    sendMock.mockResolvedValueOnce({
      session: {
        id: "daemon-session-3",
        acpId: "acp-session-3",
      },
    })

    const { runAgent } = await import("../../src/daemon/session/client.js")

    const explicitClient = {
      send: sendMock,
      subscribe: subscribeMock,
    }

    const session = await runAgent(
      {
        sessionId: "daemon-session-3",
        agent: "pi",
        cwd: "/tmp/project",
        mcpServers: [],
        systemPrompt: "Follow the spec.",
      },
      undefined,
      {
        client: explicitClient,
      },
    )

    expect(session?.sessionId).toBe("daemon-session-3")
    expect(createDaemonIpcClientFromEnvMock).not.toHaveBeenCalled()
    expect(sendMock).toHaveBeenCalledWith("sessionConnect", { id: "daemon-session-3" })
  })
})
