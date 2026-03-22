import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { createTauriTransport, IPC_STREAM_EVENT } from "../src/transport.ts"

// Mock the Tauri APIs
vi.mock(
  "@tauri-apps/api/core",
  async (importOriginal): Promise<typeof import("@tauri-apps/api/core")> => {
    const actual = await importOriginal<typeof import("@tauri-apps/api/core")>()
    return {
      ...actual,
      invoke: vi.fn<typeof actual.invoke>(),
    }
  },
)

vi.mock(
  "@tauri-apps/api/event",
  async (importOriginal): Promise<typeof import("@tauri-apps/api/event")> => {
    const actual = await importOriginal<typeof import("@tauri-apps/api/event")>()
    return {
      ...actual,
      listen: vi.fn<typeof actual.listen>(),
    }
  },
)

describe("createTauriTransport", () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  it("send forwards the right invoke payload", async () => {
    const transport = createTauriTransport("/tmp/socket.sock")
    await transport.send("my-event", { data: 123 })

    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith("plugin:ipc|send", {
      socketPath: "/tmp/socket.sock",
      name: "my-event",
      payload: { data: 123 },
    })
  })

  it("subscribe filters messages correctly and unlisten does not leak", async () => {
    const unlistenMock: Awaited<ReturnType<typeof listen>> = vi.fn(async () => undefined)
    vi.mocked(listen).mockResolvedValue(unlistenMock)
    vi.mocked(invoke).mockResolvedValue("sub-123" as never)

    const transport = createTauriTransport("/tmp/socket.sock")
    const onMessage = vi.fn()

    const unsubscribe = await transport.subscribe("my-event", onMessage)

    expect(listen).toHaveBeenCalledTimes(1)
    expect(listen).toHaveBeenCalledWith(IPC_STREAM_EVENT, expect.any(Function))

    // Capture the registered event handler
    const eventHandler = vi.mocked(listen).mock.calls[0]![1] as Function

    // Emulate an event that matches
    eventHandler({
      payload: {
        subscriptionId: "sub-123",
        socketPath: "/tmp/socket.sock",
        name: "my-event",
        payload: "match-payload",
      },
    })
    expect(onMessage).toHaveBeenCalledWith("match-payload")
    onMessage.mockClear()

    // Emulate an event with wrong subscriptionId
    eventHandler({
      payload: {
        subscriptionId: "sub-456",
        socketPath: "/tmp/socket.sock",
        name: "my-event",
        payload: "wrong-sub",
      },
    })
    expect(onMessage).not.toHaveBeenCalled()

    // Emulate an event with wrong socketPath
    eventHandler({
      payload: {
        subscriptionId: "sub-123",
        socketPath: "/other.sock",
        name: "my-event",
        payload: "wrong-socket",
      },
    })
    expect(onMessage).not.toHaveBeenCalled()

    // Emulate an event with wrong name
    eventHandler({
      payload: {
        subscriptionId: "sub-123",
        socketPath: "/tmp/socket.sock",
        name: "other-event",
        payload: "wrong-name",
      },
    })
    expect(onMessage).not.toHaveBeenCalled()

    // Unsubscribe
    await unsubscribe()

    expect(unlistenMock).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith("plugin:ipc|unsubscribe", { subscriptionId: "sub-123" })
  })

  it("failed subscribe cleanup calls unlisten and does not leak listeners", async () => {
    const unlistenMock: Awaited<ReturnType<typeof listen>> = vi.fn(async () => undefined)
    vi.mocked(listen).mockResolvedValue(unlistenMock)
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Plugin error"))

    const transport = createTauriTransport("/tmp/socket.sock")

    await expect(transport.subscribe("my-event", vi.fn())).rejects.toThrow("Plugin error")

    expect(unlistenMock).toHaveBeenCalledTimes(1)
  })
})
