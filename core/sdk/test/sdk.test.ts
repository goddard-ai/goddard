import * as acp from "acp-client/protocol"
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
  const client = createMockRouteClient(send, subscribe)
  const sdk = new GoddardSdk({
    client,
  })

  return { sdk, send, subscribe }
}

function createMockRouteClient(
  send: ReturnType<typeof vi.fn>,
  subscribe: ReturnType<typeof vi.fn>,
): GoddardClient {
  const base = {}

  return new Proxy(base, {
    get(target, property) {
      if (property in target) {
        return target[property as keyof typeof target]
      }

      return createMockRouteNode([String(property)], subscribe, send)
    },
  }) as GoddardClient
}

function createMockRouteNode(
  path: readonly string[],
  subscribe: ReturnType<typeof vi.fn>,
  send: ReturnType<typeof vi.fn>,
): unknown {
  const route = async (input?: unknown, options?: { signal?: AbortSignal }) => {
    const name = path.join(".")
    if (options?.signal) {
      return createMockStream(name, input, options.signal, subscribe)
    }

    return send(name, input)
  }

  return new Proxy(route, {
    get(_, property) {
      if (property === "then") {
        return undefined
      }

      return createMockRouteNode([...path, String(property)], subscribe, send)
    },
  })
}

async function* createMockStream(
  name: string,
  input: unknown,
  signal: AbortSignal,
  subscribe: ReturnType<typeof vi.fn>,
) {
  const queue: unknown[] = []
  let notify: (() => void) | undefined
  const target = input === undefined ? name : { name, filter: input }
  const unsubscribe = await subscribe(target, (payload: unknown) => {
    queue.push(payload)
    notify?.()
  })
  signal.addEventListener("abort", () => {
    notify?.()
  })

  try {
    while (!signal.aborted) {
      const payload = queue.shift()
      if (payload !== undefined) {
        yield payload
        continue
      }
      await new Promise<void>((resolve) => {
        notify = resolve
      })
    }
  } finally {
    unsubscribe()
  }
}

describe("@goddard-ai/sdk session namespace", () => {
  test("assigns daemon and feature namespaces during construction", () => {
    const { sdk } = createSdkWithClient()

    expect(Object.hasOwn(sdk, "daemon")).toBe(true)
    expect(Object.hasOwn(sdk, "auth")).toBe(true)
    expect(Object.hasOwn(sdk, "adapter")).toBe(true)
    expect(Object.hasOwn(sdk, "pr")).toBe(true)
    expect(Object.hasOwn(sdk, "inbox")).toBe(true)
    expect(Object.hasOwn(sdk, "session")).toBe(true)
    expect(Object.hasOwn(sdk, "reviewSession")).toBe(true)
    expect(Object.hasOwn(sdk, "action")).toBe(true)
    expect(Object.hasOwn(sdk, "loop")).toBe(true)
    expect(Object.hasOwn(sdk, "workforce")).toBe(true)
  })

  test("inbox.streamItems streams daemon inbox item updates", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const unsubscribe = vi.fn()
    const controller = new AbortController()

    subscribe.mockImplementationOnce(
      async (target: unknown, handler: (payload: unknown) => void) => {
        expect(target).toBe("inbox.streamItems")
        handler({
          id: "inb_1",
          entityId: "ses_1",
          reason: "session.turn_ended",
          status: "unread",
          priority: "normal",
          updatedAt: 1,
          readAt: null,
          scope: "Checkout flow",
          headline: "Review needed",
          turnId: "turn-1",
        })
        return unsubscribe
      },
    )

    const events = await sdk.inbox.streamItems(undefined, { signal: controller.signal })
    const iterator = events[Symbol.asyncIterator]()
    const result = await iterator.next()

    expect(subscribe).toHaveBeenCalledWith("inbox.streamItems", expect.any(Function))
    expect(result.value).toEqual({
      id: "inb_1",
      entityId: "ses_1",
      reason: "session.turn_ended",
      status: "unread",
      priority: "normal",
      updatedAt: 1,
      readAt: null,
      scope: "Checkout flow",
      headline: "Review needed",
      turnId: "turn-1",
    })
    controller.abort()
    await iterator.return?.()
    expect(unsubscribe).toHaveBeenCalledTimes(1)
  })

  test("adapter.list forwards to adapter.list", async () => {
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

    expect(send).toHaveBeenCalledWith("adapter.list", { cwd: "/tmp/project" })
  })

  test("session.changes forwards to session.changes", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      id: "ses_1",
      acpSessionId: "acp-session-1",
      workspaceRoot: "/repo",
      diff: "diff --git a/file.ts b/file.ts\n",
      hasChanges: true,
    })

    await expect(sdk.session.changes({ id: "ses_1" })).resolves.toEqual({
      id: "ses_1",
      acpSessionId: "acp-session-1",
      workspaceRoot: "/repo",
      diff: "diff --git a/file.ts b/file.ts\n",
      hasChanges: true,
    })

    expect(send).toHaveBeenCalledWith("session.changes", {
      id: "ses_1",
    })
  })

  test("session.send forwards ACP messages to session.send", async () => {
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

    expect(send).toHaveBeenCalledWith("session.send", {
      id: "ses_1",
      message,
    })
  })

  test("session.cancel forwards daemon-owned turn cancellation to session.cancel", async () => {
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

    expect(send).toHaveBeenCalledWith("session.cancel", {
      id: "ses_daemon-session-1",
    })
  })

  test("session.steer forwards one replacement prompt to session.steer", async () => {
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

    expect(send).toHaveBeenCalledWith("session.steer", {
      id: "ses_daemon-session-1",
      prompt: "Review only the failing tests.",
    })
  })

  test("reviewSession routes forward the expected daemon requests", async () => {
    const { sdk, send } = createSdkWithClient()
    const reviewSession = {
      sessionId: "review-session-1",
      agentWorktree: "/repo/wt",
      reviewWorktree: "/repo",
      agentBranch: "goddard-ses_1",
      reviewBranch: "review-sync/goddard-ses_1",
      paused: false,
      refs: {
        agentSnapshot: "refs/review-sync/review-session-1/agent-snapshot",
        renderedSnapshot: "refs/review-sync/review-session-1/rendered-snapshot",
      },
      agentSnapshot: "abc123",
      renderedSnapshot: "def456",
      lastSync: {
        status: "synced",
        acceptedPatch: null,
        rejectedPatch: null,
      },
      patchCounts: {
        accepted: 1,
        rejected: 0,
      },
    }

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
      },
      reviewSession,
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
      },
      reviewSession,
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
      },
      reviewSession: null,
      warnings: [],
    })

    await expect(sdk.reviewSession.mount({ id: "ses_1" })).resolves.toMatchObject({
      reviewSession,
    })
    await expect(sdk.reviewSession.run({ id: "ses_1" })).resolves.toMatchObject({
      reviewSession,
    })
    await expect(sdk.reviewSession.unmount({ id: "ses_1" })).resolves.toMatchObject({
      reviewSession: null,
    })

    expect(send).toHaveBeenNthCalledWith(1, "reviewSession.mount", {
      id: "ses_1",
    })
    expect(send).toHaveBeenNthCalledWith(2, "reviewSession.run", {
      id: "ses_1",
    })
    expect(send).toHaveBeenNthCalledWith(3, "reviewSession.unmount", {
      id: "ses_1",
    })
  })

  test("session.streamMessages streams daemon-side session messages", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const unsubscribe = vi.fn()
    const controller = new AbortController()

    subscribe.mockImplementationOnce(
      async (target: unknown, handler: (payload: unknown) => void) => {
        expect(target).toEqual({
          name: "session.streamMessages",
          filter: { id: "ses_1" },
        })
        handler({
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params: { value: "kept" },
        })
        return unsubscribe
      },
    )

    const events = await sdk.session.streamMessages({ id: "ses_1" }, { signal: controller.signal })
    const iterator = events[Symbol.asyncIterator]()
    const result = await iterator.next()

    expect(subscribe).toHaveBeenCalledWith(
      { name: "session.streamMessages", filter: { id: "ses_1" } },
      expect.any(Function),
    )
    expect(result.value).toEqual({
      jsonrpc: "2.0",
      method: acp.CLIENT_METHODS.session_update,
      params: { value: "kept" },
    })
    controller.abort()
    await iterator.return?.()
    expect(unsubscribe).toHaveBeenCalledTimes(1)
  })

  test("session.streamLifecycle streams daemon session lifecycle events", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const unsubscribe = vi.fn()
    const controller = new AbortController()

    subscribe.mockImplementationOnce(
      async (target: unknown, handler: (payload: unknown) => void) => {
        expect(target).toBe("session.streamLifecycle")
        handler({
          kind: "sessionUpdated",
          session: {
            id: "ses_1",
            createdAt: 1,
            updatedAt: 2,
            acpSessionId: "acp_1",
            status: "done",
            stopReason: "end_turn",
            agent: null,
            agentName: "Agent",
            cwd: "/repo",
            title: "Session",
            titleState: "generated",
            mcpServers: [],
            connectionMode: "history",
            supportsLoadSession: true,
            activeDaemonSession: false,
            completedHidden: false,
            errorMessage: null,
            blockedReason: null,
            initiative: null,
            inboxScope: null,
            lastAgentMessage: "Finished",
            repository: null,
            prNumber: null,
            token: null,
            permissions: null,
            metadata: null,
            models: null,
            configOptions: [],
            availableCommands: [],
            contextUsage: null,
          },
          changed: ["status", "connection"],
        })
        return unsubscribe
      },
    )

    const events = await sdk.session.streamLifecycle(undefined, { signal: controller.signal })
    const iterator = events[Symbol.asyncIterator]()
    const result = await iterator.next()

    expect(subscribe).toHaveBeenCalledWith("session.streamLifecycle", expect.any(Function))
    expect(result.value).toMatchObject({
      kind: "sessionUpdated",
      session: { id: "ses_1", status: "done" },
      changed: ["status", "connection"],
    })
    controller.abort()
    await iterator.return?.()
    expect(unsubscribe).toHaveBeenCalledTimes(1)
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

    expect(send).toHaveBeenCalledWith("session.composerSuggestions", {
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

    expect(send).toHaveBeenCalledWith("session.draftSuggestions", {
      cwd: "/repo",
      trigger: "dollar",
      query: "check",
    })
  })

  test("session.launchPreview forwards launch capability inspection requests", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      launchLeaseId: "lease_1",
      repoRoot: "/repo",
      bare: false,
      branches: ["main", "feature-a"],
      currentBranch: "main",
      dirty: false,
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
      launchLeaseId: "lease_1",
      repoRoot: "/repo",
      bare: false,
      branches: ["main", "feature-a"],
      currentBranch: "main",
      dirty: false,
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

    expect(send).toHaveBeenCalledWith("session.launchPreview", {
      agent: "pi-acp",
      cwd: "/repo",
    })
  })

  test("session.launchLease.release forwards launch lease release requests", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      launchLeaseId: "lease_1",
      released: true,
    })

    await expect(
      sdk.session.launchLease.release({
        launchLeaseId: "lease_1",
      }),
    ).resolves.toEqual({
      launchLeaseId: "lease_1",
      released: true,
    })

    expect(send).toHaveBeenCalledWith("session.launchLease.release", {
      launchLeaseId: "lease_1",
    })
  })

  test("session.subpackages forwards launch working directory discovery requests", async () => {
    const { sdk, send } = createSdkWithClient()

    send.mockResolvedValueOnce({
      subpackages: [
        {
          path: "/repo/packages/app",
          relativePath: "packages/app",
          name: "app",
          manifestPath: "/repo/packages/app/package.json",
        },
      ],
    })

    await expect(
      sdk.session.subpackages({
        cwd: "/repo",
      }),
    ).resolves.toEqual({
      subpackages: [
        {
          path: "/repo/packages/app",
          relativePath: "packages/app",
          name: "app",
          manifestPath: "/repo/packages/app/package.json",
        },
      ],
    })

    expect(send).toHaveBeenCalledWith("session.subpackages", {
      cwd: "/repo",
    })
  })

  test("deriveSessionLaunchModelConfig defaults invalid current models to the first available model", () => {
    const launchModelConfig = deriveSessionLaunchModelConfig({
      models: {
        currentModelId: "removed-model",
        availableModels: [
          {
            modelId: "gpt-5.4",
            name: "GPT-5.4",
          },
          {
            modelId: "gpt-5.4-mini",
            name: "GPT-5.4 Mini",
          },
        ],
      },
      configOptions: [],
    })

    expect(launchModelConfig.models?.currentModelId).toBe("gpt-5.4")
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

  test("deriveSessionLaunchModelConfig folds slash-delimited thinking model ids", () => {
    const launchModelConfig = deriveSessionLaunchModelConfig({
      models: {
        currentModelId: "gpt-5.5/high",
        availableModels: [
          {
            modelId: "gpt-5.5/low",
            name: "GPT-5.5",
          },
          {
            modelId: "gpt-5.5/high",
            name: "GPT-5.5",
          },
          {
            modelId: "gpt-5.5-mini/low",
            name: "GPT-5.5 Mini",
          },
          {
            modelId: "gpt-5.5-mini/high",
            name: "GPT-5.5 Mini",
          },
        ],
      },
      configOptions: [],
    })

    expect(launchModelConfig.models).toEqual({
      currentModelId: "__goddard_model_0_gpt-5-5",
      availableModels: [
        {
          modelId: "__goddard_model_0_gpt-5-5",
          name: "GPT-5.5",
          description: undefined,
        },
        {
          modelId: "__goddard_model_1_gpt-5-5-mini",
          name: "GPT-5.5 Mini",
          description: undefined,
        },
      ],
    })
    expect(launchModelConfig.configOptions).toContainEqual(
      expect.objectContaining({
        category: "thought_level",
        currentValue: "high",
        options: [
          { value: "low", name: "Low" },
          { value: "high", name: "High" },
        ],
      }),
    )
    expect(
      launchModelConfig.resolveSelection({
        modelId: launchModelConfig.models?.availableModels[1]?.modelId,
        configOptions: [
          {
            configId: "_goddard_derived_thinking_level",
            value: "low",
          },
        ],
      }),
    ).toEqual({
      initialModelId: "gpt-5.5-mini/low",
      initialConfigOptions: undefined,
    })
  })

  test("deriveSessionLaunchModelConfig folds thinking suffixes with explicit ACP thinking config options", () => {
    const input = {
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

    expect(launchModelConfig.models).toEqual({
      currentModelId: "__goddard_model_0_gpt-5-4",
      availableModels: [
        {
          modelId: "__goddard_model_0_gpt-5-4",
          name: "GPT-5.4",
          description: "Balanced frontier model",
        },
      ],
    })
    expect(launchModelConfig.configOptions).toEqual(input.configOptions)
    expect(
      launchModelConfig.resolveSelection({
        modelId: launchModelConfig.models?.availableModels[0]?.modelId,
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

    expect(send).toHaveBeenNthCalledWith(1, "session.create", {
      agent: "pi-acp",
      cwd: "/tmp/project",
      localCheckout: undefined,
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
      { name: "session.streamMessages", filter: { id: "ses_1" } },
      expect.any(Function),
    )
    expect(send).toHaveBeenNthCalledWith(2, "session.shutdown", { id: "ses_1" })
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

    expect(send).toHaveBeenNthCalledWith(1, "session.create", {
      agent: undefined,
      cwd: "/tmp/project",
      localCheckout: undefined,
      worktree: undefined,
      mcpServers: [],
      systemPrompt: undefined,
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

  test("workforce.streamEvents streams ledger events", async () => {
    const { sdk, subscribe } = createSdkWithClient()
    const unsubscribe = vi.fn()
    const controller = new AbortController()

    subscribe.mockImplementationOnce(
      async (target: unknown, handler: (payload: unknown) => void) => {
        expect(target).toEqual({
          name: "workforce.streamEvents",
          filter: { rootDir: "/repo" },
        })
        handler({
          id: "evt-1",
          at: "2026-03-31T00:00:00.000Z",
          type: "request",
          requestId: "req-1",
          toAgentId: "root",
          fromAgentId: null,
          intent: "default",
          input: "Review the queue.",
        })
        return unsubscribe
      },
    )

    const events = await sdk.workforce.streamEvents(
      { rootDir: "/repo" },
      { signal: controller.signal },
    )
    const iterator = events[Symbol.asyncIterator]()
    const result = await iterator.next()

    expect(subscribe).toHaveBeenCalledWith(
      { name: "workforce.streamEvents", filter: { rootDir: "/repo" } },
      expect.any(Function),
    )
    expect(result.value).toEqual({
      id: "evt-1",
      at: "2026-03-31T00:00:00.000Z",
      type: "request",
      requestId: "req-1",
      toAgentId: "root",
      fromAgentId: null,
      intent: "default",
      input: "Review the queue.",
    })
    controller.abort()
    await iterator.return?.()
    expect(unsubscribe).toHaveBeenCalledTimes(1)
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
    const cancel = vi.fn().mockResolvedValueOnce({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [],
    })
    const session = new AgentSession(
      "ses_daemon-session-1",
      "acp-session-1",
      {} as never,
      {
        session: {
          cancel,
        },
      } as never,
      vi.fn(),
    )

    await expect(session.cancel()).resolves.toEqual({
      id: "ses_daemon-session-1",
      activeTurnCancelled: true,
      abortedQueue: [],
    })

    expect(cancel).toHaveBeenCalledWith({
      id: "ses_daemon-session-1",
    })
  })

  test("AgentSession.steer uses the daemon-owned steer path", async () => {
    const steer = vi.fn().mockResolvedValueOnce({
      id: "ses_daemon-session-1",
      abortedQueue: [],
      response: { stopReason: "end_turn" },
    })
    const session = new AgentSession(
      "ses_daemon-session-1",
      "acp-session-1",
      {} as never,
      {
        session: {
          steer,
        },
      } as never,
      vi.fn(),
    )

    await expect(session.steer("Focus on the lint failure.")).resolves.toEqual({
      id: "ses_daemon-session-1",
      abortedQueue: [],
      response: { stopReason: "end_turn" },
    })

    expect(steer).toHaveBeenCalledWith({
      id: "ses_daemon-session-1",
      prompt: "Focus on the lint failure.",
    })
  })
})
