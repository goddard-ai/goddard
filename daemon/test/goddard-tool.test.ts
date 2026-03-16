import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"

const {
  createDaemonRouteClientFromEnvMock,
  getByServerIdMock,
  prReplyRoutePostMock,
  prSubmitRoutePostMock,
  updateMock,
} = vi.hoisted(() => ({
  getByServerIdMock: vi.fn(async () => ({ id: "session-7" })),
  updateMock: vi.fn(async () => undefined),
  prSubmitRoutePostMock: vi.fn(async () => ({
    number: 12,
    url: "https://github.com/acme/widgets/pull/12",
  })),
  prReplyRoutePostMock: vi.fn(async () => ({ success: true })),
  createDaemonRouteClientFromEnvMock: vi.fn(() => ({
    sessionToken: "tok_session",
    client: {
      prSubmitRoute: {
        POST: prSubmitRoutePostMock,
      },
      prReplyRoute: {
        POST: prReplyRoutePostMock,
      },
    },
  })),
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    getByServerId: getByServerIdMock,
    update: updateMock,
  },
}))

vi.mock("../src/client.ts", () => ({
  createDaemonRouteClientFromEnv: createDaemonRouteClientFromEnvMock,
}))

import { declareInitiative, main, reportBlocker, reportCompleted } from "../src/bin/goddard-tool.ts"

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
      GODDARD_SERVER_ID: "server-7",
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
      GODDARD_SESSION_TOKEN: "tok_session",
    }

    getByServerIdMock.mockClear()
    updateMock.mockClear()
    prSubmitRoutePostMock.mockClear()
    prReplyRoutePostMock.mockClear()
    createDaemonRouteClientFromEnvMock.mockClear()
  })

  afterEach(async () => {
    process.env = previousEnv
    await rm(tempDir, { recursive: true, force: true })
  })

  test("declare initiative stores latest initiative", async () => {
    await declareInitiative("session-1", "Ship websocket cancellation")

    expect(updateMock).toHaveBeenCalledWith("session-1", {
      initiative: "Ship websocket cancellation",
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

  test("resolves session id from GODDARD_SERVER_ID", async () => {
    await main(["declare-initiative", "--title", "Ship server id lookup"])

    expect(getByServerIdMock).toHaveBeenCalledWith("server-7")
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      initiative: "Ship server id lookup",
      blockedReason: null,
      status: "active",
    })
  })

  test("submit-pr uses daemon route client before updating session state", async () => {
    await main(["submit-pr", "--title", "Ship daemon IPC", "--body-file", emptyFile])

    expect(createDaemonRouteClientFromEnvMock).toHaveBeenCalledTimes(1)
    expect(prSubmitRoutePostMock).toHaveBeenCalledWith({
      headers: { authorization: "Bearer tok_session" },
      body: {
        cwd: process.cwd(),
        title: "Ship daemon IPC",
        body: "",
      },
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      status: "done",
      lastAgentMessage:
        "PR Submitted: Ship daemon IPC\nhttps://github.com/acme/widgets/pull/12\n\n",
    })
  })

  test("reply-pr uses daemon route client before updating session state", async () => {
    await main(["reply-pr", "--message-file", emptyFile])

    expect(createDaemonRouteClientFromEnvMock).toHaveBeenCalledTimes(1)
    expect(prReplyRoutePostMock).toHaveBeenCalledWith({
      headers: { authorization: "Bearer tok_session" },
      body: {
        cwd: process.cwd(),
        message: "",
      },
    })
    expect(updateMock).toHaveBeenCalledWith("session-7", {
      status: "done",
      lastAgentMessage: "PR Reply: ",
    })
  })
})
