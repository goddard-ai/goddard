import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { mkdtemp, writeFile, rm } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"

const { getByServerIdMock, updateMock, submitPrViaDaemonMock, replyPrViaDaemonMock } = vi.hoisted(() => ({
  getByServerIdMock: vi.fn(async () => ({ id: "session-7" })),
  updateMock: vi.fn(async () => undefined),
  submitPrViaDaemonMock: vi.fn(async () => ({
    number: 12,
    url: "https://github.com/acme/widgets/pull/12",
  })),
  replyPrViaDaemonMock: vi.fn(async () => ({ success: true })),
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    getByServerId: getByServerIdMock,
    update: updateMock,
  },
}))

vi.mock("../src/bin/daemon-client.ts", () => ({
  submitPrViaDaemon: submitPrViaDaemonMock,
  replyPrViaDaemon: replyPrViaDaemonMock,
}))

import { main } from "../src/bin/goddard-tool.js"

describe("goddard tool session resolution", () => {
  const previousEnv = process.env
  let tempDir = ""
  let emptyFile = ""

  beforeEach(async () => {
    tempDir = await mkdtemp(join(tmpdir(), "goddard-session-tool-"))
    emptyFile = join(tempDir, "empty.txt")
    await writeFile(emptyFile, "", "utf-8")

    process.env = {
      ...previousEnv,
      GODDARD_SERVER_ID: "server-7",
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
    }
    getByServerIdMock.mockClear()
    updateMock.mockClear()
    submitPrViaDaemonMock.mockClear()
    replyPrViaDaemonMock.mockClear()
  })

  afterEach(async () => {
    process.env = previousEnv
    await rm(tempDir, { recursive: true, force: true })
  })

  test("resolves session id from GODDARD_SERVER_ID", async () => {
    await main(["declare-initiative", "--title", "Ship server id lookup"])

    expect(getByServerIdMock).toHaveBeenCalledWith("server-7")
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      initiative: "Ship server id lookup",
      blockedReason: null,
      status: "active",
    })
  })

  test("submit-pr routes through the daemon before updating session state", async () => {
    await main(["submit-pr", "--title", "Ship daemon IPC", "--body-file", emptyFile])

    expect(submitPrViaDaemonMock).toHaveBeenCalledWith({
      cwd: process.cwd(),
      title: "Ship daemon IPC",
      body: "",
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      status: "done",
      lastAgentMessage: "PR Submitted: Ship daemon IPC\nhttps://github.com/acme/widgets/pull/12\n\n",
    })
  })

  test("reply-pr routes through the daemon before updating session state", async () => {
    await main(["reply-pr", "--message-file", emptyFile])

    expect(replyPrViaDaemonMock).toHaveBeenCalledWith({
      cwd: process.cwd(),
      message: "",
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      status: "done",
      lastAgentMessage: "PR Reply: ",
    })
  })
})
