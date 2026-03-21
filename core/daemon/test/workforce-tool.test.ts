import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { afterEach, beforeEach, describe, expect, mock, test, vi } from "bun:test"

const sendMock = vi.fn(async (_name: string) => ({ requestId: "req-1" }))
const createDaemonIpcClientFromEnvMock = vi.fn(() => ({
  client: {
    send: sendMock,
  },
}))
const actualDaemonClient = await import("../client/src/index.ts")

mock.module("@goddard-ai/daemon-client", () => ({
  ...actualDaemonClient,
  createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
}))

const {
  main,
  workforceCancel,
  workforceRequest,
  workforceRespond,
  workforceSuspend,
  workforceTruncate,
  workforceUpdate,
} = await import("../src/bin/workforce-tool.ts")
mock.restore()

describe("daemon workforce tool", () => {
  const previousEnv = process.env
  let tempDir = ""
  let emptyFile = ""

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "goddard-workforce-tool-"))
    emptyFile = join(tempDir, "empty.txt")
    await writeFile(emptyFile, "", "utf-8")

    process.env = {
      ...previousEnv,
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
      GODDARD_SESSION_TOKEN: "tok_session",
      GODDARD_WORKFORCE_ROOT_DIR: "/repo",
      GODDARD_WORKFORCE_REQUEST_ID: "req-env",
    }

    sendMock.mockClear()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  afterEach(async () => {
    process.env = previousEnv
    await rm(tempDir, { recursive: true, force: true })
  })

  test("workforce request delegates through daemon IPC", async () => {
    await workforceRequest("api", "Ship it.")

    expect(sendMock).toHaveBeenCalledWith("workforceRequest", {
      rootDir: "/repo",
      targetAgentId: "api",
      input: "Ship it.",
      token: "tok_session",
    })
  })

  test("workforce update delegates through daemon IPC", async () => {
    await workforceUpdate("req-1", "Resume it.")

    expect(sendMock).toHaveBeenCalledWith("workforceUpdate", {
      rootDir: "/repo",
      requestId: "req-1",
      input: "Resume it.",
      token: "tok_session",
    })
  })

  test("workforce cancel and truncate delegate through daemon IPC", async () => {
    await workforceCancel("req-1", "Not needed.")
    await workforceTruncate("api", "Reset queue.")

    expect(sendMock).toHaveBeenNthCalledWith(1, "workforceCancel", {
      rootDir: "/repo",
      requestId: "req-1",
      reason: "Not needed.",
      token: "tok_session",
    })
    expect(sendMock).toHaveBeenNthCalledWith(2, "workforceTruncate", {
      rootDir: "/repo",
      agentId: "api",
      reason: "Reset queue.",
      token: "tok_session",
    })
  })

  test("workforce respond and suspend require the session token", async () => {
    await workforceRespond("req-1", "Done.")
    await workforceSuspend("req-1", "Need a root decision.")

    expect(sendMock).toHaveBeenNthCalledWith(1, "workforceRespond", {
      rootDir: "/repo",
      requestId: "req-1",
      output: "Done.",
      token: "tok_session",
    })
    expect(sendMock).toHaveBeenNthCalledWith(2, "workforceSuspend", {
      rootDir: "/repo",
      requestId: "req-1",
      reason: "Need a root decision.",
      token: "tok_session",
    })
  })

  test("main routes request and respond subcommands", async () => {
    await main(["request", "--target-agent-id", "api", "--input-file", emptyFile])
    await main(["respond", "--output-file", emptyFile])

    expect(sendMock).toHaveBeenNthCalledWith(1, "workforceRequest", {
      rootDir: "/repo",
      targetAgentId: "api",
      input: "",
      token: "tok_session",
    })
    expect(sendMock).toHaveBeenNthCalledWith(2, "workforceRespond", {
      rootDir: "/repo",
      requestId: "req-env",
      output: "",
      token: "tok_session",
    })
  })
})
