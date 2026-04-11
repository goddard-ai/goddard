import * as acp from "@agentclientprotocol/sdk"
import { describe, expect, test, vi } from "bun:test"
import { AgentSession, GoddardSdk, type GoddardClient } from "../src/index.ts"

function createSdkWithClient() {
  const send = vi.fn()
  const subscribe = vi.fn()
  const client: GoddardClient = {
    send: (name, payload?) => send(name, payload),
    subscribe: (target, onMessage) => subscribe(target, onMessage),
  }
  const sdk = new GoddardSdk({
    client,
  })

  return { sdk, send, subscribe }
}

describe("@goddard-ai/sdk session namespace", () => {
  test("adapter.list forwards to adapterList", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      adapters: [],
      defaultAdapterId: "pi-acp",
      registrySource: "cache",
      lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
      stale: false,
      lastError: null,
    })

    await expect(sdk.adapter.list({ cwd: "/tmp/project" })).resolves.toEqual({
      adapters: [],
      defaultAdapterId: "pi-acp",
      registrySource: "cache",
      lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
      stale: false,
      lastError: null,
    })

    expect(send).toHaveBeenCalledWith("adapterList", { cwd: "/tmp/project" })
  })

  test("session.send forwards ACP messages to sessionSend", async () => {
    const { sdk, send } = createSdkWithClient()
    const message: acp.AnyMessage = {
      jsonrpc: "2.0",
      id: "prompt-1",
      method: acp.AGENT_METHODS.session_prompt,
      params: {
        sessionId: "acp-session-1",
        prompt: [{ type: "text", text: "Review the diff." }],
      },
    }

    send.mockResolvedValueOnce({ accepted: true })

    await expect(sdk.session.send({ id: "ses_1", message })).resolves.toEqual({
      accepted: true,
    })

    expect(send).toHaveBeenCalledWith("sessionSend", {
      id: "ses_1",
      message,
    })
  })

  test("session.cancel forwards daemon-owned turn cancellation to sessionCancel", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [
        { requestId: "prompt-2", prompt: [{ type: "text", text: "Queued follow-up" }] },
      ],
    })

    await expect(sdk.session.cancel({ id: "ses_daemon-session-1" })).resolves.toEqual({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [
        { requestId: "prompt-2", prompt: [{ type: "text", text: "Queued follow-up" }] },
      ],
    })

    expect(send).toHaveBeenCalledWith("sessionCancel", { id: "ses_daemon-session-1" })
  })

  test("session.steer forwards one replacement prompt to sessionSteer", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      id: "ses_daemon-session-1",
      abortedQueue: [],
      response: { stopReason: "end_turn" },
    })

    await expect(
      sdk.session.steer({
        id: "ses_daemon-session-1",
        prompt: "Review only the failing tests.",
      }),
    ).resolves.toEqual({
      id: "ses_daemon-session-1",
      abortedQueue: [],
      response: { stopReason: "end_turn" },
    })

    expect(send).toHaveBeenCalledWith("sessionSteer", {
      id: "ses_daemon-session-1",
      prompt: "Review only the failing tests.",
    })
  })

  test("session worktree sync helpers forward the expected daemon requests", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      id: "ses_1",
      acpSessionId: "acp-session-1",
      worktree: {
        repoRoot: "/repo",
        requestedCwd: "/repo",
        effectiveCwd: "/repo/wt",
        worktreeDir: "/repo/wt",
        branchName: "goddard-ses_1",
        poweredBy: "default",
        sync: null,
      },
      warnings: [],
    })
    send.mockResolvedValueOnce({
      id: "ses_1",
      acpSessionId: "acp-session-1",
      worktree: {
        repoRoot: "/repo",
        requestedCwd: "/repo",
        effectiveCwd: "/repo/wt",
        worktreeDir: "/repo/wt",
        branchName: "goddard-ses_1",
        poweredBy: "default",
        sync: null,
      },
      warnings: [],
    })
    send.mockResolvedValueOnce({
      id: "ses_1",
      acpSessionId: "acp-session-1",
      worktree: {
        repoRoot: "/repo",
        requestedCwd: "/repo",
        effectiveCwd: "/repo/wt",
        worktreeDir: "/repo/wt",
        branchName: "goddard-ses_1",
        poweredBy: "default",
        sync: null,
      },
      warnings: [],
    })

    await sdk.session.mountWorktreeSync({ id: "ses_1" })
    await sdk.session.syncWorktree({ id: "ses_1" })
    await sdk.session.unmountWorktree({ id: "ses_1" })

    expect(send).toHaveBeenNthCalledWith(1, "sessionWorktreeSyncMount", {
      id: "ses_1",
    })
    expect(send).toHaveBeenNthCalledWith(2, "sessionWorktreeSync", { id: "ses_1" })
    expect(send).toHaveBeenNthCalledWith(3, "sessionWorktreeSyncUnmount", { id: "ses_1" })
  })

  test("session.subscribe passes the daemon-side session filter and unwraps messages", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const onMessage = vi.fn()
    const unsubscribe = vi.fn()

    subscribe.mockImplementationOnce(
      async (
        target: Parameters<GoddardClient["subscribe"]>[0],
        handler: Parameters<GoddardClient["subscribe"]>[1],
      ) => {
        expect(target).toEqual({ name: "sessionMessage", filter: { id: "ses_1" } })
        handler({
          id: "ses_1",
          message: {
            jsonrpc: "2.0",
            method: acp.CLIENT_METHODS.session_update,
            params: { value: "kept" },
          },
        })
        return unsubscribe
      },
    )

    const result = await sdk.session.subscribe({ id: "ses_1" }, onMessage)

    expect(subscribe).toHaveBeenCalledWith(
      { name: "sessionMessage", filter: { id: "ses_1" } },
      expect.any(Function),
    )
    expect(onMessage).toHaveBeenCalledTimes(1)
    expect(onMessage).toHaveBeenCalledWith({
      jsonrpc: "2.0",
      method: acp.CLIENT_METHODS.session_update,
      params: { value: "kept" },
    })
    expect(result).toBe(unsubscribe)
  })

  test("session.run returns an AgentSession", async () => {
    const { sdk, send, subscribe } = createSdkWithClient()
    const unsubscribe = vi.fn()

    subscribe.mockResolvedValueOnce(unsubscribe)
    send.mockResolvedValueOnce({
      session: {
        id: "ses_1",
        acpSessionId: "acp-session-1",
      },
    })
    send.mockResolvedValueOnce({ id: "ses_1", success: true })

    const session = await sdk.session.run({
      agent: "pi-acp",
      cwd: "/tmp/project",
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    })

    expect(session).toBeInstanceOf(AgentSession)
    await session!.stop()

    expect(send).toHaveBeenNthCalledWith(1, "sessionCreate", {
      agent: "pi-acp",
      cwd: "/tmp/project",
      worktree: undefined,
      mcpServers: [],
      systemPrompt: "Keep responses short.",
      env: undefined,
      repository: undefined,
      prNumber: undefined,
      metadata: undefined,
      initialPrompt: undefined,
      oneShot: undefined,
    })
    expect(subscribe).toHaveBeenCalledWith(
      { name: "sessionMessage", filter: { id: "ses_1" } },
      expect.any(Function),
    )
    expect(send).toHaveBeenNthCalledWith(2, "sessionShutdown", { id: "ses_1" })
  })

  test("workforce.subscribe passes the root filter and unwraps ledger events", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const onEvent = vi.fn()
    const unsubscribe = vi.fn()

    subscribe.mockImplementationOnce(
      async (
        target: Parameters<GoddardClient["subscribe"]>[0],
        handler: Parameters<GoddardClient["subscribe"]>[1],
      ) => {
        expect(target).toEqual({ name: "workforceEvent", filter: { rootDir: "/repo" } })
        handler({
          rootDir: "/repo",
          event: {
            id: "evt-1",
            at: "2026-03-31T00:00:00.000Z",
            type: "request",
            requestId: "req-1",
            toAgentId: "root",
            fromAgentId: null,
            intent: "default",
            input: "Review the queue.",
          },
        })
        return unsubscribe
      },
    )

    const result = await sdk.workforce.subscribe({ rootDir: "/repo" }, onEvent)

    expect(subscribe).toHaveBeenCalledWith(
      { name: "workforceEvent", filter: { rootDir: "/repo" } },
      expect.any(Function),
    )
    expect(onEvent).toHaveBeenCalledTimes(1)
    expect(onEvent).toHaveBeenCalledWith({
      id: "evt-1",
      at: "2026-03-31T00:00:00.000Z",
      type: "request",
      requestId: "req-1",
      toAgentId: "root",
      fromAgentId: null,
      intent: "default",
      input: "Review the queue.",
    })
    expect(result).toBe(unsubscribe)
  })

  test("AgentSession.setAgentModel forwards the requested model id through ACP", async () => {
    const setModelMock = vi.fn()
    const session = new AgentSession(
      "ses_1",
      "acp-session-1",
      {
        unstable_setSessionModel: setModelMock,
      } as never,
      {
        send: vi.fn(),
      } as never,
      vi.fn(),
    )

    await session.setAgentModel("gpt-5.4")

    expect(setModelMock).toHaveBeenCalledWith({
      sessionId: "acp-session-1",
      modelId: "gpt-5.4",
    })
  })

  test("AgentSession.cancel uses the daemon-owned cancel path", async () => {
    const daemonSend = vi.fn().mockResolvedValueOnce({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [],
    })
    const session = new AgentSession(
      "ses_daemon-session-1",
      "acp-session-1",
      {} as never,
      {
        send: daemonSend,
      } as never,
      vi.fn(),
    )

    await expect(session.cancel()).resolves.toEqual({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [],
    })

    expect(daemonSend).toHaveBeenCalledWith("sessionCancel", { id: "ses_daemon-session-1" })
  })

  test("AgentSession.steer uses the daemon-owned steer path", async () => {
    const daemonSend = vi.fn().mockResolvedValueOnce({
      id: "ses_daemon-session-1",
      abortedQueue: [],
      response: { stopReason: "end_turn" },
    })
    const session = new AgentSession(
      "ses_daemon-session-1",
      "acp-session-1",
      {} as never,
      {
        send: daemonSend,
      } as never,
      vi.fn(),
    )

    await expect(session.steer("Focus on the lint failure.")).resolves.toEqual({
      id: "ses_daemon-session-1",
      abortedQueue: [],
      response: { stopReason: "end_turn" },
    })

    expect(daemonSend).toHaveBeenCalledWith("sessionSteer", {
      id: "ses_daemon-session-1",
      prompt: "Focus on the lint failure.",
    })
  })
})
