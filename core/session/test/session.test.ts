import os from "node:os"
import path from "node:path"

import { describe, expect, it } from "vitest"

import {
  createRandomSocketPath,
  getDefaultIpcDirectory,
  resolveServerListenTarget,
  type ServerEndpoint,
} from "../src/transport"

describe("transport", () => {
  it("uses an explicitly provided tcp port", () => {
    expect(resolveServerListenTarget({ port: 4321 })).toEqual({
      kind: "tcp",
      value: 4321,
      display: "ws://localhost:4321",
    })
  })

  it("uses a provided Unix socket path", () => {
    expect(resolveServerListenTarget({ socketPath: "/tmp/goddard.sock" })).toEqual({
      kind: "ipc",
      value: "/tmp/goddard.sock",
      display: "ws+unix:///tmp/goddard.sock",
    })
  })

  it("uses an ephemeral tcp port by default", () => {
    expect(resolveServerListenTarget()).toEqual({
      kind: "tcp",
      value: 0,
      display: "ws://localhost:0",
    })
  })

  it("builds a randomized Unix socket path when requested", () => {
    const socketPath = createRandomSocketPath("linux")
    expect(socketPath.startsWith(path.join(os.tmpdir(), "goddard-session-"))).toBe(true)
    expect(socketPath.endsWith(".sock")).toBe(true)
  })

  it("uses a randomized ipc path when ipc transport is requested without a socket path", () => {
    const target = resolveServerListenTarget({ transport: "ipc" })
    expect(target.kind).toBe("ipc")
    expect(target.display.startsWith("ws+unix://")).toBe(true)
  })

  it("uses the OS temp dir for Unix IPC sockets", () => {
    expect(getDefaultIpcDirectory("linux")).toBe(os.tmpdir())
  })

  it("returns a websocket URL from the actual bound tcp port", () => {
    const endpoint: ServerEndpoint = { kind: "tcp", port: 54321, url: "ws://localhost:54321" }
    expect(endpoint.url).toBe("ws://localhost:54321")
  })

  it("returns a websocket URL from the actual bound IPC path", () => {
    const endpoint: ServerEndpoint = {
      kind: "ipc",
      socketPath: "/tmp/goddard.sock",
      url: "ws+unix:///tmp/goddard.sock",
    }
    expect(endpoint.url).toBe("ws+unix:///tmp/goddard.sock")
  })
})
