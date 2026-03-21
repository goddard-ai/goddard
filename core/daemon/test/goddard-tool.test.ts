import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { afterEach, beforeEach, describe, expect, mock, test, vi } from "bun:test"

const updateMock = vi.fn(async () => undefined)
const sendMock = vi.fn(async (name: string) => {
  if (name === "sessionResolveToken") {
    return { id: "session-7" }
  }
  if (name === "prSubmit") {
    return {
      number: 12,
      url: "https://github.com/acme/widgets/pull/12",
    }
  }
  if (name === "prReply") {
    return { success: true }
  }
  return { ok: true }
})
const createDaemonIpcClientFromEnvMock = vi.fn(() => ({
  client: {
    send: sendMock,
  },
}))
const actualStorage = await import("../../storage/src/index.ts")
const actualDaemonClient = await import("../client/src/index.ts")

mock.module("@goddard-ai/storage", () => ({
  ...actualStorage,
  SessionStorage: {
    update: updateMock,
  },
}))

mock.module("@goddard-ai/daemon-client", () => ({
  ...actualDaemonClient,
  createDaemonIpcClientFromEnv: createDaemonIpcClientFromEnvMock,
}))

const { declareInitiative, main, reportBlocker, reportCompleted } =
  await import("../src/bin/goddard-tool.ts")
mock.restore()

describe("daemon goddard tool", () => {
  const previousEnv = process.env
  let tempDir = ""
  let emptyFile = ""

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "goddard-daemon-tool-"))
    emptyFile = join(tempDir, "empty.txt")
    await writeFile(emptyFile, "", "utf-8")

    process.env = {
      ...previousEnv,
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
      GODDARD_SESSION_TOKEN: "tok_session",
    }

    updateMock.mockClear()
    sendMock.mockClear()
    createDaemonIpcClientFromEnvMock.mockClear()
  })

  afterEach(async () => {
    process.env = previousEnv
    await rm(tempDir, { recursive: true, force: true })
  })

  test("declare initiative stores latest initiative", async () => {
    await declareInitiative("session-1", "Ship IPC token resolution")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      initiative: "Ship IPC token resolution",
      blockedReason: null,
      status: "active",
    })
  })

  test("report blocker stores reason and blocked status", async () => {
    await reportBlocker("session-1", "Needs maintainer review")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      status: "blocked",
      blockedReason: "Needs maintainer review",
    })
  })

  test("report completed clears initiative and returns session to done", async () => {
    await reportCompleted("session-1")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      initiative: null,
      blockedReason: null,
      status: "done",
    })
  })

  test("resolves session id from GODDARD_SESSION_TOKEN via daemon", async () => {
    await main(["declare-initiative", "--title", "Ship token lookup"])

    expect(sendMock).toHaveBeenCalledWith("sessionResolveToken", {
      token: "tok_session",
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      initiative: "Ship token lookup",
      blockedReason: null,
      status: "active",
    })
  })

  test("submit-pr uses daemon IPC before updating session state", async () => {
    await main(["submit-pr", "--title", "Ship daemon IPC", "--body-file", emptyFile])

    expect(createDaemonIpcClientFromEnvMock).toHaveBeenCalledTimes(2)
    expect(sendMock).toHaveBeenCalledWith("prSubmit", {
      token: "tok_session",
      cwd: process.cwd(),
      title: "Ship daemon IPC",
      body: "",
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      status: "done",
      lastAgentMessage:
        "PR Submitted: Ship daemon IPC\nhttps://github.com/acme/widgets/pull/12\n\n",
    })
  })

  test("reply-pr uses daemon IPC before updating session state", async () => {
    await main(["reply-pr", "--message-file", emptyFile])

    expect(createDaemonIpcClientFromEnvMock).toHaveBeenCalledTimes(2)
    expect(sendMock).toHaveBeenCalledWith("prReply", {
      token: "tok_session",
      cwd: process.cwd(),
      message: "",
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      status: "done",
      lastAgentMessage: "PR Reply: ",
    })
  })
})
