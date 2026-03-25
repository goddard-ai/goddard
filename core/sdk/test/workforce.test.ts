import { assert, beforeEach, test, vi } from "vitest"
import * as workforce from "../src/node/workforce.ts"

const {
  cancelDaemonWorkforceRequestMock,
  createDaemonWorkforceRequestMock,
  getDaemonWorkforceMock,
  listDaemonWorkforcesMock,
  shutdownDaemonWorkforceMock,
  startDaemonWorkforceMock,
  truncateDaemonWorkforceMock,
  updateDaemonWorkforceRequestMock,
} = vi.hoisted(() => ({
  cancelDaemonWorkforceRequestMock: vi.fn(),
  createDaemonWorkforceRequestMock: vi.fn(),
  getDaemonWorkforceMock: vi.fn(),
  listDaemonWorkforcesMock: vi.fn(),
  shutdownDaemonWorkforceMock: vi.fn(),
  startDaemonWorkforceMock: vi.fn(),
  truncateDaemonWorkforceMock: vi.fn(),
  updateDaemonWorkforceRequestMock: vi.fn(),
}))

vi.mock(
  "../src/daemon/workforce.js",
  async (importOriginal): Promise<typeof import("../src/daemon/workforce.ts")> => ({
    ...(await importOriginal<typeof import("../src/daemon/workforce.ts")>()),
    cancelDaemonWorkforceRequest: cancelDaemonWorkforceRequestMock,
    createDaemonWorkforceRequest: createDaemonWorkforceRequestMock,
    getDaemonWorkforce: getDaemonWorkforceMock,
    listDaemonWorkforces: listDaemonWorkforcesMock,
    shutdownDaemonWorkforce: shutdownDaemonWorkforceMock,
    startDaemonWorkforce: startDaemonWorkforceMock,
    truncateDaemonWorkforce: truncateDaemonWorkforceMock,
    updateDaemonWorkforceRequest: updateDaemonWorkforceRequestMock,
  }),
)

beforeEach(() => {
  cancelDaemonWorkforceRequestMock.mockReset()
  createDaemonWorkforceRequestMock.mockReset()
  getDaemonWorkforceMock.mockReset()
  listDaemonWorkforcesMock.mockReset()
  shutdownDaemonWorkforceMock.mockReset()
  startDaemonWorkforceMock.mockReset()
  truncateDaemonWorkforceMock.mockReset()
  updateDaemonWorkforceRequestMock.mockReset()
})

test("node workforce helpers delegate lifecycle and request mutations to daemon wrappers", async () => {
  startDaemonWorkforceMock.mockResolvedValue({ rootDir: "/repo" })
  getDaemonWorkforceMock.mockResolvedValue({ rootDir: "/repo" })
  listDaemonWorkforcesMock.mockResolvedValue([{ rootDir: "/repo" }])
  shutdownDaemonWorkforceMock.mockResolvedValue(true)
  createDaemonWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  updateDaemonWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  cancelDaemonWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  truncateDaemonWorkforceMock.mockResolvedValue({ requestId: null })

  await workforce.startWorkforce("/repo", {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
  })
  await workforce.getWorkforce("/repo")
  await workforce.listWorkforces()
  await workforce.stopWorkforce("/repo")
  await workforce.createWorkforceRequest({
    rootDir: "/repo",
    targetAgentId: "api",
    message: "Ship the change.",
  })
  await workforce.updateWorkforceRequest({
    rootDir: "/repo",
    requestId: "req-1",
    message: "Resume with the new decision.",
  })
  await workforce.cancelWorkforceRequest({
    rootDir: "/repo",
    requestId: "req-1",
    reason: "No longer needed.",
  })
  await workforce.truncateWorkforce({
    rootDir: "/repo",
    agentId: "api",
    reason: "Reset the failed branch.",
  })

  assert.equal(startDaemonWorkforceMock.mock.calls.length, 1)
  assert.equal(getDaemonWorkforceMock.mock.calls.length, 1)
  assert.equal(listDaemonWorkforcesMock.mock.calls.length, 1)
  assert.equal(shutdownDaemonWorkforceMock.mock.calls.length, 1)
  assert.equal(createDaemonWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(updateDaemonWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(cancelDaemonWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(truncateDaemonWorkforceMock.mock.calls.length, 1)
})
