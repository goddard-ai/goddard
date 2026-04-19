import * as acp from "@agentclientprotocol/sdk"
import { describe, expect, test, vi } from "bun:test"

import {
  AgentSession,
  deriveSessionLaunchModelConfig,
  GoddardSdk,
  type GoddardClient,
} from "../src/index.ts"

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
        {
          requestId: "prompt-2",
          prompt: [{ type: "text", text: "Queued follow-up" }],
        },
      ],
    })

    await expect(sdk.session.cancel({ id: "ses_daemon-session-1" })).resolves.toEqual({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [
        {
          requestId: "prompt-2",
          prompt: [{ type: "text", text: "Queued follow-up" }],
        },
      ],
    })

    expect(send).toHaveBeenCalledWith("sessionCancel", {
      id: "ses_daemon-session-1",
    })
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
    expect(send).toHaveBeenNthCalledWith(2, "sessionWorktreeSync", {
      id: "ses_1",
    })
    expect(send).toHaveBeenNthCalledWith(3, "sessionWorktreeSyncUnmount", {
      id: "ses_1",
    })
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
        expect(target).toEqual({
          name: "sessionMessage",
          filter: { id: "ses_1" },
        })
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

  test("session.composerSuggestions forwards session-scoped suggestion reads", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      suggestions: [
        {
          type: "file",
          path: "/repo/src/index.ts",
          uri: "file:///repo/src/index.ts",
          label: "index.ts",
          detail: "./src/index.ts",
        },
      ],
    })

    await expect(
      sdk.session.composerSuggestions({
        id: "ses_1",
        trigger: "at",
        query: "index",
      }),
    ).resolves.toEqual({
      suggestions: [
        {
          type: "file",
          path: "/repo/src/index.ts",
          uri: "file:///repo/src/index.ts",
          label: "index.ts",
          detail: "./src/index.ts",
        },
      ],
    })

    expect(send).toHaveBeenCalledWith("sessionComposerSuggestions", {
      id: "ses_1",
      trigger: "at",
      query: "index",
    })
  })

  test("session.draftSuggestions forwards launch-dialog suggestion reads", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      suggestions: [
        {
          type: "skill",
          path: "/repo/.agents/skills/checks/SKILL.md",
          uri: "file:///repo/.agents/skills/checks/SKILL.md",
          label: "checks",
          detail: "./.agents/skills/checks/SKILL.md",
          source: "local",
        },
      ],
    })

    await expect(
      sdk.session.draftSuggestions({
        cwd: "/repo",
        trigger: "dollar",
        query: "check",
      }),
    ).resolves.toEqual({
      suggestions: [
        {
          type: "skill",
          path: "/repo/.agents/skills/checks/SKILL.md",
          uri: "file:///repo/.agents/skills/checks/SKILL.md",
          label: "checks",
          detail: "./.agents/skills/checks/SKILL.md",
          source: "local",
        },
      ],
    })

    expect(send).toHaveBeenCalledWith("sessionDraftSuggestions", {
      cwd: "/repo",
      trigger: "dollar",
      query: "check",
    })
  })

  test("session.launchPreview forwards launch capability inspection requests", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      repoRoot: "/repo",
      branches: [
        { name: "main", current: true },
        { name: "feature-a", current: false },
      ],
      models: {
        currentModelId: "gpt-5.4",
        availableModels: [
          {
            modelId: "gpt-5.4",
            name: "GPT-5.4",
            description: "Balanced frontier model",
          },
        ],
      },
      configOptions: [],
      slashCommands: [
        {
          type: "slash_command",
          name: "plan",
          description: "Create or revise the plan",
          inputHint: "What should change?",
        },
      ],
    })

    await expect(
      sdk.session.launchPreview({
        agent: "pi-acp",
        cwd: "/repo",
      }),
    ).resolves.toEqual({
      repoRoot: "/repo",
      branches: [
        { name: "main", current: true },
        { name: "feature-a", current: false },
      ],
      models: {
        currentModelId: "gpt-5.4",
        availableModels: [
          {
            modelId: "gpt-5.4",
            name: "GPT-5.4",
            description: "Balanced frontier model",
          },
        ],
      },
      configOptions: [],
      slashCommands: [
        {
          type: "slash_command",
          name: "plan",
          description: "Create or revise the plan",
          inputHint: "What should change?",
        },
      ],
    })

    expect(send).toHaveBeenCalledWith("sessionLaunchPreview", {
      agent: "pi-acp",
      cwd: "/repo",
    })
  })

  test("deriveSessionLaunchModelConfig folds thinking suffixes into one selector", () => {
    const launchModelConfig = deriveSessionLaunchModelConfig({
      models: {
        currentModelId: "gpt-5.4-medium",
        availableModels: [
          {
            modelId: "gpt-5.4-low",
            name: "GPT-5.4 (Low)",
            description: "Balanced frontier model",
          },
          {
            modelId: "gpt-5.4-medium",
            name: "GPT-5.4 (Medium)",
            description: "Balanced frontier model",
          },
          {
            modelId: "gpt-5.4-high",
            name: "GPT-5.4 (High)",
            description: "Balanced frontier model",
          },
          {
            modelId: "gpt-5.4-mini-low",
            name: "GPT-5.4 Mini (Low)",
            description: "Faster lower-latency variant",
          },
          {
            modelId: "gpt-5.4-mini-medium",
            name: "GPT-5.4 Mini (Medium)",
            description: "Faster lower-latency variant",
          },
          {
            modelId: "gpt-5.4-mini-high",
            name: "GPT-5.4 Mini (High)",
            description: "Faster lower-latency variant",
          },
        ],
      },
      configOptions: [],
    })

    expect(launchModelConfig.models).toEqual({
      currentModelId: "__goddard_model_0_gpt-5-4",
      availableModels: [
        {
          modelId: "__goddard_model_0_gpt-5-4",
          name: "GPT-5.4",
          description: "Balanced frontier model",
        },
        {
          modelId: "__goddard_model_1_gpt-5-4-mini",
          name: "GPT-5.4 Mini",
          description: "Faster lower-latency variant",
        },
      ],
    })
    expect(launchModelConfig.configOptions).toEqual([
      {
        id: "_goddard_derived_thinking_level",
        type: "select",
        name: "Thinking level",
        category: "thought_level",
        description: "Derived from ACP model names.",
        currentValue: "medium",
        options: [
          { value: "low", name: "Low" },
          { value: "medium", name: "Medium" },
          { value: "high", name: "High" },
        ],
      },
    ])
    expect(
      launchModelConfig.resolveSelection({
        modelId: launchModelConfig.models?.availableModels[1]?.modelId,
        configOptions: [
          {
            configId: "_goddard_derived_thinking_level",
            value: "high",
          },
        ],
      }),
    ).toEqual({
      initialModelId: "gpt-5.4-mini-high",
      initialConfigOptions: undefined,
    })
  })

  test("deriveSessionLaunchModelConfig preserves explicit ACP thinking config options", () => {
    const input = {
      models: {
        currentModelId: "gpt-5.4-medium",
        availableModels: [
          {
            modelId: "gpt-5.4-medium",
            name: "GPT-5.4 (Medium)",
            description: "Balanced frontier model",
          },
          {
            modelId: "gpt-5.4-high",
            name: "GPT-5.4 (High)",
            description: "Balanced frontier model",
          },
        ],
      },
      configOptions: [
        {
          id: "thinking",
          type: "select" as const,
          name: "Thinking level",
          category: "thought_level",
          currentValue: "medium",
          options: [
            { value: "medium", name: "Medium" },
            { value: "high", name: "High" },
          ],
        },
      ],
    }

    const launchModelConfig = deriveSessionLaunchModelConfig(input)

    expect(launchModelConfig.models).toEqual(input.models)
    expect(launchModelConfig.configOptions).toEqual(input.configOptions)
    expect(
      launchModelConfig.resolveSelection({
        modelId: "gpt-5.4-high",
        configOptions: [
          {
            configId: "thinking",
            value: "high",
          },
        ],
      }),
    ).toEqual({
      initialModelId: "gpt-5.4-high",
      initialConfigOptions: [
        {
          configId: "thinking",
          value: "high",
        },
      ],
    })
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
      initialModelId: "gpt-5.4-mini",
      initialConfigOptions: [
        {
          configId: "thinking",
          value: "high",
        },
      ],
    })

    expect(session).toBeInstanceOf(AgentSession)
    await session!.stop()

    expect(send).toHaveBeenNthCalledWith(1, "sessionCreate", {
      agent: "pi-acp",
      cwd: "/tmp/project",
      worktree: undefined,
      mcpServers: [],
      systemPrompt: "Keep responses short.",
      initialModelId: "gpt-5.4-mini",
      initialConfigOptions: [
        {
          configId: "thinking",
          value: "high",
        },
      ],
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

  test("session.run lets the daemon resolve the default agent when none is provided", async () => {
    const { sdk, send, subscribe } = createSdkWithClient()
    const unsubscribe = vi.fn()

    subscribe.mockResolvedValueOnce(unsubscribe)
    send.mockResolvedValueOnce({
      session: {
        id: "ses_2",
        acpSessionId: "acp-session-2",
      },
    })
    send.mockResolvedValueOnce({ id: "ses_2", success: true })

    const session = await sdk.session.run({
      cwd: "/tmp/project",
      mcpServers: [],
    })

    expect(session).toBeInstanceOf(AgentSession)
    await session!.stop()

    expect(send).toHaveBeenNthCalledWith(1, "sessionCreate", {
      agent: undefined,
      cwd: "/tmp/project",
      worktree: undefined,
      mcpServers: [],
      systemPrompt: "",
      initialModelId: undefined,
      initialConfigOptions: undefined,
      env: undefined,
      repository: undefined,
      prNumber: undefined,
      metadata: undefined,
      initialPrompt: undefined,
      oneShot: undefined,
    })
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
        expect(target).toEqual({
          name: "workforceEvent",
          filter: { rootDir: "/repo" },
        })
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

    expect(daemonSend).toHaveBeenCalledWith("sessionCancel", {
      id: "ses_daemon-session-1",
    })
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
