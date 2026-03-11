import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"

const { getByServerIdMock, updateMock } = vi.hoisted(() => ({
  getByServerIdMock: vi.fn(async () => ({ id: "session-7" })),
  updateMock: vi.fn(async () => undefined),
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    getByServerId: getByServerIdMock,
    update: updateMock,
  },
}))

import { main } from "../src/bin/goddard-tool.js"

describe("goddard tool session resolution", () => {
  const previousEnv = process.env

  beforeEach(() => {
    process.env = { ...previousEnv, GODDARD_SERVER_ID: "server-7" }
    getByServerIdMock.mockClear()
    updateMock.mockClear()
  })

  afterEach(() => {
    process.env = previousEnv
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
})
