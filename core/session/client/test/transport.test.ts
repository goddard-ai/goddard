import type { SessionEndpoint } from "@goddard-ai/session-protocol"
import { describe, expect, it } from "vitest"

import { createClientUrl } from "../src/transport"

describe("session client transport", () => {
  it("creates a websocket URL from a tcp endpoint", () => {
    const endpoint: SessionEndpoint = { kind: "tcp", port: 4321, url: "ws://localhost:4321" }
    expect(createClientUrl(endpoint)).toBe("ws://localhost:4321")
  })

  it("creates a websocket URL from a Unix socket endpoint", () => {
    const endpoint: SessionEndpoint = {
      kind: "ipc",
      socketPath: "/tmp/goddard.sock",
      url: "ws+unix:///tmp/goddard.sock",
    }
    expect(createClientUrl(endpoint)).toBe("ws+unix:///tmp/goddard.sock")
  })

  it("requires the caller to provide a server endpoint", () => {
    expect(() => createClientUrl(undefined)).toThrow("A server endpoint is required")
  })
})
