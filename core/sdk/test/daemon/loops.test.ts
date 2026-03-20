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

vi.mock("@goddard-ai/daemon-client", () => ({
  createDaemonIpcClient: vi.fn(() => ({
    send: sendMock,
  })),
  createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
}))

describe("daemon loop client", () => {
  beforeEach(() => {
    sendMock.mockReset()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  test("starts, lists, and shuts down daemon-managed loops", async () => {
    sendMock
      .mockResolvedValueOnce({ loop: { rootDir: "/repo", loopName: "review" } })
      .mockResolvedValueOnce({ loops: [{ rootDir: "/repo", loopName: "review" }] })
      .mockResolvedValueOnce({ success: true })

    const { listDaemonLoops, shutdownDaemonLoop, startDaemonLoop } =
      await import("../../src/daemon/loops.js")

    await expect(
      startDaemonLoop({
        rootDir: "/repo",
        loopName: "review",
        promptModulePath: "/repo/.goddard/loops/review/prompt.js",
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
    await expect(listDaemonLoops()).resolves.toEqual([{ rootDir: "/repo", loopName: "review" }])
    await expect(shutdownDaemonLoop("/repo", "review")).resolves.toBe(true)

    expect(sendMock).toHaveBeenNthCalledWith(1, "loopStart", {
      rootDir: "/repo",
      loopName: "review",
      promptModulePath: "/repo/.goddard/loops/review/prompt.js",
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
    expect(sendMock).toHaveBeenNthCalledWith(2, "loopList", {})
    expect(sendMock).toHaveBeenNthCalledWith(3, "loopShutdown", {
      rootDir: "/repo",
      loopName: "review",
    })
  })
})
