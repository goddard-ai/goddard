import * as assert from "node:assert/strict"
import { beforeEach, test, vi } from "vitest"

const cancelledSelection = Symbol("cancelled-selection")

const {
  cancelMock,
  cancelWorkforceRequestMock,
  createWorkforceRequestMock,
  discoverWorkforceInitCandidatesMock,
  getWorkforceMock,
  initializeWorkforceMock,
  introMock,
  isCancelMock,
  listWorkforcesMock,
  multiselectMock,
  outroMock,
  resolveRepositoryRootMock,
  startWorkforceMock,
  stopWorkforceMock,
  truncateWorkforceMock,
  updateWorkforceRequestMock,
} = vi.hoisted(() => ({
  cancelMock: vi.fn(),
  cancelWorkforceRequestMock: vi.fn(),
  createWorkforceRequestMock: vi.fn(),
  discoverWorkforceInitCandidatesMock: vi.fn(),
  getWorkforceMock: vi.fn(),
  initializeWorkforceMock: vi.fn(),
  introMock: vi.fn(),
  isCancelMock: vi.fn((value: unknown) => value === cancelledSelection),
  listWorkforcesMock: vi.fn(),
  multiselectMock: vi.fn(),
  outroMock: vi.fn(),
  resolveRepositoryRootMock: vi.fn(),
  startWorkforceMock: vi.fn(),
  stopWorkforceMock: vi.fn(),
  truncateWorkforceMock: vi.fn(),
  updateWorkforceRequestMock: vi.fn(),
}))

vi.mock("@clack/prompts", () => ({
  cancel: cancelMock,
  intro: introMock,
  isCancel: isCancelMock,
  multiselect: multiselectMock,
  outro: outroMock,
}))

vi.mock("@goddard-ai/sdk/node", () => ({
  cancelWorkforceRequest: cancelWorkforceRequestMock,
  createWorkforceRequest: createWorkforceRequestMock,
  discoverWorkforceInitCandidates: discoverWorkforceInitCandidatesMock,
  getWorkforce: getWorkforceMock,
  initializeWorkforce: initializeWorkforceMock,
  listWorkforces: listWorkforcesMock,
  resolveRepositoryRoot: resolveRepositoryRootMock,
  startWorkforce: startWorkforceMock,
  stopWorkforce: stopWorkforceMock,
  truncateWorkforce: truncateWorkforceMock,
  updateWorkforceRequest: updateWorkforceRequestMock,
}))

beforeEach(() => {
  cancelMock.mockReset()
  cancelWorkforceRequestMock.mockReset()
  createWorkforceRequestMock.mockReset()
  discoverWorkforceInitCandidatesMock.mockReset()
  getWorkforceMock.mockReset()
  initializeWorkforceMock.mockReset()
  introMock.mockReset()
  isCancelMock.mockClear()
  listWorkforcesMock.mockReset()
  multiselectMock.mockReset()
  outroMock.mockReset()
  resolveRepositoryRootMock.mockReset()
  startWorkforceMock.mockReset()
  stopWorkforceMock.mockReset()
  truncateWorkforceMock.mockReset()
  updateWorkforceRequestMock.mockReset()
})

test("init exits cleanly when there are no workforce candidates", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  discoverWorkforceInitCandidatesMock.mockResolvedValue([])
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["init", "--root", "/repo/packages/app"])

  assert.equal(consoleLog.mock.calls[0]?.[0], "No workforce candidates found under /repo.")
  assert.equal(multiselectMock.mock.calls.length, 0)
  assert.equal(initializeWorkforceMock.mock.calls.length, 0)
})

test("init cancels without creating files when the prompt is cancelled", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  discoverWorkforceInitCandidatesMock.mockResolvedValue([
    {
      rootDir: "/repo",
      relativeDir: ".",
      manifestPath: "/repo/package.json",
      name: "@repo/root",
    },
  ])
  multiselectMock.mockResolvedValue(cancelledSelection)

  const { main } = await import("../src/main.ts")
  await main(["init", "--root", "/repo"])

  assert.equal(cancelMock.mock.calls.length, 1)
  assert.equal(initializeWorkforceMock.mock.calls.length, 0)
})

test("init maps selected packages into workforce initialization", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  discoverWorkforceInitCandidatesMock.mockResolvedValue([
    {
      rootDir: "/repo",
      relativeDir: ".",
      manifestPath: "/repo/package.json",
      name: "@repo/root",
    },
    {
      rootDir: "/repo/packages/ui",
      relativeDir: "packages/ui",
      manifestPath: "/repo/packages/ui/package.json",
      name: "@repo/ui",
    },
  ])
  multiselectMock.mockResolvedValue(["/repo", "/repo/packages/ui"])
  initializeWorkforceMock.mockResolvedValue({
    configPath: "/repo/.goddard/workforce.json",
  })

  const { main } = await import("../src/main.ts")
  await main(["init", "--root", "/repo"])

  assert.deepEqual(initializeWorkforceMock.mock.calls[0]?.[0], "/repo")
  assert.deepEqual(initializeWorkforceMock.mock.calls[0]?.[1], ["/repo", "/repo/packages/ui"])
  assert.equal(outroMock.mock.calls.length, 1)
})

test("start and stop commands forward lifecycle control to the daemon-backed SDK helpers", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  startWorkforceMock.mockResolvedValue({ rootDir: "/repo" })
  stopWorkforceMock.mockResolvedValue(true)

  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})
  const { main } = await import("../src/main.ts")

  await main(["start", "--root", "/repo/packages/app"])
  await main(["stop", "--root", "/repo/packages/app"])

  assert.equal(startWorkforceMock.mock.calls.length, 1)
  assert.equal(stopWorkforceMock.mock.calls.length, 1)
  assert.equal(consoleLog.mock.calls.length, 2)
})

test("status and list print daemon-backed workforce state", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  getWorkforceMock.mockResolvedValue({ rootDir: "/repo", activeRequestCount: 0 })
  listWorkforcesMock.mockResolvedValue([{ rootDir: "/repo", activeRequestCount: 0 }])
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["status", "--root", "/repo"])
  await main(["list"])

  assert.equal(getWorkforceMock.mock.calls.length, 1)
  assert.equal(listWorkforcesMock.mock.calls.length, 1)
  assert.equal(consoleLog.mock.calls.length, 2)
})

test("request, update, and truncate commands call the matching SDK helpers", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  updateWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  truncateWorkforceMock.mockResolvedValue({ requestId: null })
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["request", "--root", "/repo", "--target-agent-id", "api", "--message", "Ship it."])
  await main(["update", "--root", "/repo", "--request-id", "req-1", "--message", "Resume it."])
  await main(["truncate", "--root", "/repo", "--agent-id", "api", "--reason", "Reset queue."])

  assert.equal(createWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(updateWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(truncateWorkforceMock.mock.calls.length, 1)
  assert.equal(consoleLog.mock.calls.length, 3)
})
