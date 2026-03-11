import { PassThrough } from "node:stream"
import { describe, expect, test, vi } from "vitest"

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    create: vi.fn(async () => undefined),
    get: vi.fn(async () => null),
    update: vi.fn(async () => undefined),
  },
}))

import { readSessionServerLog } from "../src/client.js"

describe("session server startup logs", () => {
  test("reads a successful startup log from nd-json stream", async () => {
    const output = new PassThrough()
    const logPromise = readSessionServerLog(output)

    output.write(
      `${JSON.stringify({ success: true, serverAddress: "http://localhost:3001/", serverId: "server-1", sessionId: "session-1" })}\n`,
    )

    await expect(logPromise).resolves.toEqual({
      success: true,
      serverAddress: "http://localhost:3001/",
      serverId: "server-1",
      sessionId: "session-1",
    })
  })

  test("reads a startup failure log from nd-json stream", async () => {
    const output = new PassThrough()
    const logPromise = readSessionServerLog(output)

    output.write(`${JSON.stringify({ success: false, error: "boom" })}\n`)

    await expect(logPromise).resolves.toEqual({
      success: false,
      error: "boom",
    })
  })
})
