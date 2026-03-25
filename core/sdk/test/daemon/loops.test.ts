import { beforeEach, describe, expect, test, vi } from "vitest"

const { createDaemonIpcClientFromEnvMock, sendMock } = vi.hoisted(() => {
  const sendMock = vi.fn()
  return {
    sendMock,
    createDaemonIpcClientFromEnvMock: vi.fn(() => ({
      client: {
        send: sendMock,
      },
    })),
  }
})

vi.mock(
  "@goddard-ai/daemon-client",
  async (importOriginal): Promise<typeof import("@goddard-ai/daemon-client")> => {
    const actual = await importOriginal<typeof import("@goddard-ai/daemon-client")>()
    return {
      ...actual,
      createDaemonIpcClient: vi.fn<typeof actual.createDaemonIpcClient>(() => ({
        send: sendMock,
      })),
      createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
    }
  },
)

describe("daemon loop client", () => {
  beforeEach(() => {
    sendMock.mockReset()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  test("starts named loops and delegates lifecycle helpers to daemon IPC", async () => {
    sendMock
      .mockResolvedValueOnce({ loop: { rootDir: "/repo", loopName: "review" } })
      .mockResolvedValueOnce({ loop: { rootDir: "/repo", loopName: "review" } })
      .mockResolvedValueOnce({ loop: { rootDir: "/repo", loopName: "review" } })
      .mockResolvedValueOnce({ loops: [{ rootDir: "/repo", loopName: "review" }] })
      .mockResolvedValueOnce({ success: true })

    const { getLoop, listLoops, startDaemonLoop, startNamedLoop, stopLoop } =
      await import("../../src/daemon/loops.ts")

    await expect(
      startDaemonLoop({
        rootDir: "/repo",
        loopName: "review",
        session: {
          agent: "pi-acp",
          cwd: "/repo",
          mcpServers: [],
        },
        rateLimits: {
          cycleDelay: "30s",
          maxOpsPerMinute: 4,
          maxCyclesBeforePause: 200,
        },
        retries: {
          maxAttempts: 1,
          initialDelayMs: 500,
          maxDelayMs: 5_000,
          backoffFactor: 2,
          jitterRatio: 0.2,
        },
      }),
    ).resolves.toEqual({
      rootDir: "/repo",
      loopName: "review",
    })
    await expect(
      startNamedLoop("review", {
        session: {
          cwd: "/repo",
          systemPrompt: "Use the loop checklist.",
        },
      }),
    ).resolves.toEqual({
      rootDir: "/repo",
      loopName: "review",
    })
    await expect(getLoop("/repo", "review")).resolves.toEqual({
      rootDir: "/repo",
      loopName: "review",
    })
    await expect(listLoops()).resolves.toEqual([{ rootDir: "/repo", loopName: "review" }])
    await expect(stopLoop("/repo", "review")).resolves.toBe(true)

    expect(sendMock).toHaveBeenNthCalledWith(1, "loopStart", {
      rootDir: "/repo",
      loopName: "review",
      session: {
        agent: "pi-acp",
        cwd: "/repo",
        mcpServers: [],
      },
      rateLimits: {
        cycleDelay: "30s",
        maxOpsPerMinute: 4,
        maxCyclesBeforePause: 200,
      },
      retries: {
        maxAttempts: 1,
        initialDelayMs: 500,
        maxDelayMs: 5_000,
        backoffFactor: 2,
        jitterRatio: 0.2,
      },
    })
    expect(sendMock).toHaveBeenNthCalledWith(2, "loopStart", {
      rootDir: "/repo",
      loopName: "review",
      session: {
        cwd: "/repo",
        systemPrompt: "Use the loop checklist.",
      },
      rateLimits: undefined,
      retries: undefined,
    })
    expect(sendMock).toHaveBeenNthCalledWith(3, "loopGet", {
      rootDir: "/repo",
      loopName: "review",
    })
    expect(sendMock).toHaveBeenNthCalledWith(4, "loopList", {})
    expect(sendMock).toHaveBeenNthCalledWith(5, "loopShutdown", {
      rootDir: "/repo",
      loopName: "review",
    })
  })
})
