import * as acp from "@agentclientprotocol/sdk"
import { describe, expect, test, vi } from "vitest"
import { AgentSession, GoddardSdk } from "../src/index.ts"

function createSdkWithClient() {
  const send = vi.fn()
  const subscribe = vi.fn()
  const sdk = new GoddardSdk({
    client: {
      send,
      subscribe,
    } as never,
  })

  return { sdk, send, subscribe }
}

describe("@goddard-ai/sdk session namespace", () => {
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

  test("session.subscribe passes the daemon-side session filter and unwraps messages", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const onMessage = vi.fn()
    const unsubscribe = vi.fn()

    subscribe.mockImplementationOnce(async (_name, subscription, handler) => {
      expect(subscription).toEqual({ id: "ses_1" })
      handler({
        id: "ses_1",
        message: {
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params: { value: "kept" },
        },
      })
      return unsubscribe
    })

    const result = await sdk.session.subscribe({ id: "ses_1" }, onMessage)

    expect(subscribe).toHaveBeenCalledWith("sessionMessage", { id: "ses_1" }, expect.any(Function))
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
      agent: "pi",
      cwd: "/tmp/project",
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    })

    expect(session).toBeInstanceOf(AgentSession)
    await session!.stop()

    expect(send).toHaveBeenNthCalledWith(1, "sessionCreate", {
      agent: "pi",
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
    expect(subscribe).toHaveBeenCalledWith("sessionMessage", { id: "ses_1" }, expect.any(Function))
    expect(send).toHaveBeenNthCalledWith(2, "sessionShutdown", { id: "ses_1" })
  })

  test("workforce.subscribe passes the root filter and unwraps ledger events", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const onEvent = vi.fn()
    const unsubscribe = vi.fn()

    subscribe.mockImplementationOnce(async (_name, subscription, handler) => {
      expect(subscription).toEqual({ rootDir: "/repo" })
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
    })

    const result = await sdk.workforce.subscribe({ rootDir: "/repo" }, onEvent)

    expect(subscribe).toHaveBeenCalledWith(
      "workforceEvent",
      { rootDir: "/repo" },
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
        prompt: vi.fn(),
        cancel: vi.fn(),
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
})
