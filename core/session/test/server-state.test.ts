import { describe, expect, test, vi } from "vitest"

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    update: vi.fn(async () => undefined),
  },
}))

vi.mock("radashi", () => ({
  noop: () => undefined,
  once: <T extends (...args: any[]) => any>(fn: T) => fn,
}))

import {
  buildAgentProcessEnv,
  injectSystemPrompt,
  sessionStatusFromAgentMessage,
  sessionStatusFromClientMessage,
  shouldExitAfterInitialPrompt,
} from "../src/server.js"

describe("session state transitions", () => {
  test("marks session cancelled on session/cancel client notification", async () => {
    const status = sessionStatusFromClientMessage(
      {
        jsonrpc: "2.0",
        method: "session/cancel",
        params: { sessionId: "session-1" },
      } as any,
      "active",
    )

    expect(status).toBe("cancelled")
  })

  test("marks session done only when prompt stopReason is end_turn", async () => {
    const doneStatus = sessionStatusFromAgentMessage(
      {
        jsonrpc: "2.0",
        id: 1,
        method: "session/prompt",
        params: {
          sessionId: "session-1",
          prompt: [{ type: "text", text: "hello" }],
        },
      } as any,
      {
        jsonrpc: "2.0",
        id: 1,
        result: {
          stopReason: "end_turn",
        },
      } as any,
    )

    expect(doneStatus).toBe("done")

    const cancelledStatus = sessionStatusFromAgentMessage(
      {
        jsonrpc: "2.0",
        id: 2,
        method: "session/prompt",
        params: {
          sessionId: "session-1",
          prompt: [{ type: "text", text: "hello" }],
        },
      } as any,
      {
        jsonrpc: "2.0",
        id: 2,
        result: {
          stopReason: "cancelled",
        },
      } as any,
    )

    expect(cancelledStatus).toBeNull()
  })
})

describe("system prompt injection", () => {
  test("prepends system prompt to a session/prompt payload", () => {
    const systemPrompt = '<system-prompt name="Goddard CLI">base</system-prompt>'
    const request = {
      sessionId: "session-1",
      prompt: [{ type: "text", text: "Hello" }],
    } as any

    const injected = injectSystemPrompt(request, systemPrompt)

    expect((injected.prompt[0] as { text: string }).text).toContain(
      '<system-prompt name="Goddard CLI">',
    )
    expect((injected.prompt[1] as { text: string }).text).toBe("Hello")
  })

  test("inserts appended system prompt after the default prompt", () => {
    const request = {
      sessionId: "session-1",
      prompt: [{ type: "text", text: "Hello" }],
    } as any

    const injected = injectSystemPrompt(
      request,
      '<system-prompt name="Goddard CLI">base</system-prompt>',
      "Follow internal policy",
    )

    expect((injected.prompt[0] as { text: string }).text).toContain(
      '<system-prompt name="Goddard CLI">base</system-prompt>',
    )
    expect((injected.prompt[1] as { text: string }).text).toBe(
      "<system-prompt>Follow internal policy</system-prompt>",
    )
    expect((injected.prompt[2] as { text: string }).text).toBe("Hello")
  })

  test("inserts multiple appended system prompts in order", () => {
    const request = {
      sessionId: "session-1",
      prompt: [{ type: "text", text: "Hello" }],
    } as any

    const injected = injectSystemPrompt(
      request,
      '<system-prompt name="Goddard CLI">base</system-prompt>',
      ["Follow internal policy", "Use repository conventions"],
    )

    expect((injected.prompt[0] as { text: string }).text).toContain(
      '<system-prompt name="Goddard CLI">base</system-prompt>',
    )
    expect((injected.prompt[1] as { text: string }).text).toBe(
      "<system-prompt>Follow internal policy</system-prompt>",
    )
    expect((injected.prompt[2] as { text: string }).text).toBe(
      "<system-prompt>Use repository conventions</system-prompt>",
    )
    expect((injected.prompt[3] as { text: string }).text).toBe("Hello")
  })

  test("flattens nested appended system prompts and drops falsy values", () => {
    const request = {
      sessionId: "session-1",
      prompt: [{ type: "text", text: "Hello" }],
    } as any

    const injected = injectSystemPrompt(
      request,
      '<system-prompt name="Goddard CLI">base</system-prompt>',
      ["Follow internal policy", null, ["Use repository conventions", "", false]],
    )

    expect((injected.prompt[0] as { text: string }).text).toContain(
      '<system-prompt name="Goddard CLI">base</system-prompt>',
    )
    expect((injected.prompt[1] as { text: string }).text).toBe(
      "<system-prompt>Follow internal policy</system-prompt>",
    )
    expect((injected.prompt[2] as { text: string }).text).toBe(
      "<system-prompt>Use repository conventions</system-prompt>",
    )
    expect((injected.prompt[3] as { text: string }).text).toBe("Hello")
  })
})

describe("one-shot session behavior", () => {
  test("exits early only when oneShot and initialPrompt are both provided", () => {
    expect(
      shouldExitAfterInitialPrompt({
        agent: "pi",
        cwd: "/tmp",
        mcpServers: [],
        initialPrompt: "hello",
        oneShot: true,
      }),
    ).toBe(true)

    expect(
      shouldExitAfterInitialPrompt({
        agent: "pi",
        cwd: "/tmp",
        mcpServers: [],
        initialPrompt: "hello",
      }),
    ).toBe(false)

    expect(
      shouldExitAfterInitialPrompt({
        agent: "pi",
        cwd: "/tmp",
        mcpServers: [],
        sessionId: "existing-session",
      }),
    ).toBe(false)
  })
})

describe("agent process environment", () => {
  test("preserves caller-provided PATH without using GODDARD_AGENT_BIN_DIR", () => {
    const env = buildAgentProcessEnv("server-1", {
      PATH: "/daemon/agent-bin:/usr/bin",
      GODDARD_AGENT_BIN_DIR: "/should/not/be/used",
      CUSTOM_VAR: "value",
    })

    expect(env.PATH).toBe("/daemon/agent-bin:/usr/bin")
    expect(env.GODDARD_AGENT_BIN_DIR).toBe("/should/not/be/used")
    expect(env.CUSTOM_VAR).toBe("value")
    expect(env.GODDARD_SERVER_ID).toBe("server-1")
  })
})
