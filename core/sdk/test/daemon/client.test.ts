import { beforeEach, describe, expect, test, vi } from "vitest"

const { createDaemonIpcClientFromEnvMock, unsubscribeMock, sendMock, subscribeMock } = vi.hoisted(
  () => {
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
  },
)

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
  function buildSession(id: string, acpId: string) {
    return {
      id,
      acpId,
      status: "active" as const,
      agentName: "pi",
      cwd: "/tmp/project",
      repository: null,
      prNumber: null,
      metadata: null,
      connection: {
        mode: "live" as const,
        reconnectable: true,
        historyAvailable: true,
        activeDaemonSession: true,
      },
      diagnostics: {
        eventCount: 0,
        historyLength: 0,
        lastEventAt: null,
      },
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      errorMessage: null,
      blockedReason: null,
      initiative: null,
      lastAgentMessage: null,
    }
  }

  beforeEach(() => {
    sendMock.mockReset()
    subscribeMock.mockClear()
    unsubscribeMock.mockClear()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  test("creates one-shot daemon sessions and returns null", async () => {
    sendMock.mockResolvedValueOnce({
      session: buildSession("daemon-session-1", "acp-session-1"),
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
      worktree: undefined,
      mcpServers: [],
      systemPrompt: "Follow the spec.",
      env: undefined,
      repository: undefined,
      prNumber: undefined,
      metadata: undefined,
      initialPrompt: "Ship it",
      oneShot: true,
    })
    expect(subscribeMock).not.toHaveBeenCalled()
  })

  test("forwards worktree opt-in to daemon session creation", async () => {
    sendMock.mockResolvedValueOnce({
      session: buildSession("daemon-session-5", "acp-session-5"),
    })

    const { runAgent } = await import("../../src/daemon/session/client.js")

    await expect(
      runAgent({
        agent: "pi",
        cwd: "/tmp/project",
        worktree: { enabled: true },
        mcpServers: [],
        systemPrompt: "Follow the spec.",
        initialPrompt: "Ship it",
        oneShot: true,
      }),
    ).resolves.toBeNull()

    expect(sendMock).toHaveBeenCalledWith("sessionCreate", {
      agent: "pi",
      cwd: "/tmp/project",
      worktree: { enabled: true },
      mcpServers: [],
      systemPrompt: "Follow the spec.",
      env: undefined,
      repository: undefined,
      prNumber: undefined,
      metadata: undefined,
      initialPrompt: "Ship it",
      oneShot: true,
    })
  })

  test("connects to daemon-hosted sessions and delegates history/shutdown over IPC", async () => {
    sendMock
      .mockResolvedValueOnce({
        session: buildSession("daemon-session-2", "acp-session-2"),
      })
      .mockResolvedValueOnce({
        id: "daemon-session-2",
        acpId: "acp-session-2",
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: true,
          activeDaemonSession: true,
        },
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
      session: buildSession("daemon-session-3", "acp-session-3"),
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

  test("can inspect daemon session connectivity before attempting reconnect", async () => {
    sendMock.mockResolvedValueOnce({
      session: {
        ...buildSession("daemon-session-4", "acp-session-4"),
        connection: {
          mode: "history" as const,
          reconnectable: false,
          historyAvailable: true,
          activeDaemonSession: false,
        },
      },
    })

    const { getDaemonSession } = await import("../../src/daemon/session/client.js")
    const session = await getDaemonSession("daemon-session-4")

    expect(session.connection.mode).toBe("history")
    expect(session.connection.reconnectable).toBe(false)
    expect(sendMock).toHaveBeenCalledWith("sessionGet", { id: "daemon-session-4" })
  })

  test("lists recent daemon sessions with pagination params", async () => {
    sendMock.mockResolvedValueOnce({
      sessions: [buildSession("daemon-session-5", "acp-session-5")],
      nextCursor: "cursor-1",
      hasMore: true,
    })

    const { listDaemonSessions } = await import("../../src/daemon/session/client.js")
    const response = await listDaemonSessions({ limit: 10, cursor: "cursor-0" })

    expect(response.sessions).toHaveLength(1)
    expect(response.nextCursor).toBe("cursor-1")
    expect(response.hasMore).toBe(true)
    expect(sendMock).toHaveBeenCalledWith("sessionList", {
      limit: 10,
      cursor: "cursor-0",
    })
  })
})
