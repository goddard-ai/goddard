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

vi.mock("@goddard-ai/daemon-client", async (importOriginal): Promise<typeof import("@goddard-ai/daemon-client")> => ({
  ...(await importOriginal<typeof import("@goddard-ai/daemon-client")>()),
  createDaemonIpcClient: vi.fn(() => ({
    send: sendMock,
  })) as any,
  createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
}))

describe("daemon workforce client", () => {
  beforeEach(() => {
    sendMock.mockReset()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  test("starts, lists, and shuts down daemon-managed workforces", async () => {
    sendMock
      .mockResolvedValueOnce({ workforce: { rootDir: "/repo" } })
      .mockResolvedValueOnce({ workforces: [{ rootDir: "/repo" }] })
      .mockResolvedValueOnce({ success: true })

    const { startDaemonWorkforce, listDaemonWorkforces, shutdownDaemonWorkforce } =
      await import("../../src/daemon/workforce.js")

    await expect(startDaemonWorkforce("/repo")).resolves.toEqual({ rootDir: "/repo" })
    await expect(listDaemonWorkforces()).resolves.toEqual([{ rootDir: "/repo" }])
    await expect(shutdownDaemonWorkforce("/repo")).resolves.toBe(true)

    expect(sendMock).toHaveBeenNthCalledWith(1, "workforceStart", { rootDir: "/repo" })
    expect(sendMock).toHaveBeenNthCalledWith(2, "workforceList", {})
    expect(sendMock).toHaveBeenNthCalledWith(3, "workforceShutdown", { rootDir: "/repo" })
  })

  test("forwards create-intent workforce requests to daemon IPC", async () => {
    sendMock.mockResolvedValueOnce({
      workforce: { rootDir: "/repo" },
      requestId: "req-create-1",
    })

    const { createDaemonWorkforceRequest } = await import("../../src/daemon/workforce.js")

    await expect(
      createDaemonWorkforceRequest({
        rootDir: "/repo",
        targetAgentId: "root",
        message: "Create a new worker package.",
        intent: "create",
      }),
    ).resolves.toEqual({
      workforce: { rootDir: "/repo" },
      requestId: "req-create-1",
    })

    expect(sendMock).toHaveBeenNthCalledWith(1, "workforceRequest", {
      rootDir: "/repo",
      targetAgentId: "root",
      input: "Create a new worker package.",
      intent: "create",
    })
  })
})
