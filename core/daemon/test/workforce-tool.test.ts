import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"

const { createDaemonIpcClientFromEnvMock, sendMock } = vi.hoisted(() => ({
  sendMock: vi.fn(async (_name: string) => ({ requestId: "req-1" })),
  createDaemonIpcClientFromEnvMock: vi.fn(() => ({
    client: {
      send: sendMock,
    },
  })),
}))

vi.mock("@goddard-ai/daemon-client", () => ({
  createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
}))

import {
  main,
  workforceCancel,
  workforceRequest,
  workforceRespond,
  workforceSuspend,
  workforceTruncate,
  workforceUpdate,
} from "../src/bin/workforce-tool.ts"

describe("daemon workforce tool", () => {
  const previousEnv = process.env
  let tempDir = ""
  let emptyFile = ""
  let logSpy: ReturnType<typeof vi.spyOn>

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "goddard-workforce-tool-"))
    emptyFile = join(tempDir, "empty.txt")
    await writeFile(emptyFile, "", "utf-8")
    logSpy = vi.spyOn(console, "log").mockImplementation(() => {})

    process.env = {
      ...previousEnv,
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
      GODDARD_SESSION_TOKEN: "tok_session",
      GODDARD_WORKFORCE_ROOT_DIR: "/repo",
    }

    sendMock.mockClear()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  afterEach(async () => {
    logSpy.mockRestore()
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

  test("workforce respond and suspend delegate through daemon IPC without request ids", async () => {
    await workforceRespond("Done.")
    await workforceSuspend("Need a root decision.")

    expect(sendMock).toHaveBeenNthCalledWith(1, "workforceRespond", {
      rootDir: "/repo",
      output: "Done.",
      token: "tok_session",
    })
    expect(sendMock).toHaveBeenNthCalledWith(2, "workforceSuspend", {
      rootDir: "/repo",
      reason: "Need a root decision.",
      token: "tok_session",
    })
  })

  test("workforce respond and suspend reject missing session tokens", async () => {
    delete process.env.GODDARD_SESSION_TOKEN

    await expect(workforceRespond("Done.")).rejects.toThrow("GODDARD_SESSION_TOKEN is required")
    await expect(workforceSuspend("Need a root decision.")).rejects.toThrow(
      "GODDARD_SESSION_TOKEN is required",
    )
    expect(sendMock).not.toHaveBeenCalled()
  })

  test("main routes request and respond subcommands without success logs", async () => {
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
      output: "",
      token: "tok_session",
    })
    expect(logSpy).not.toHaveBeenCalled()
  })
})
