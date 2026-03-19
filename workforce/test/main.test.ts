import { afterEach, assert, beforeEach, test, vi } from "vitest"

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

afterEach(() => {
  vi.restoreAllMocks()
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
  assert.match(String(consoleLog.mock.calls[0]?.[0]), /Started workforce for \/repo\./)
  assert.match(String(consoleLog.mock.calls[1]?.[0]), /Stopped workforce for \/repo\./)
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
  assert.match(String(consoleLog.mock.calls[0]?.[0]), /"rootDir": "\/repo"/)
  assert.match(String(consoleLog.mock.calls[1]?.[0]), /"activeRequestCount": 0/)
})

test("request defaults to the root agent and update/truncate call the matching SDK helpers", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  updateWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  truncateWorkforceMock.mockResolvedValue({ requestId: null })
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["request", "--root", "/repo", "--message", "Ship it."])
  await main(["update", "--root", "/repo", "--request-id", "req-1", "--message", "Resume it."])
  await main(["truncate", "--root", "/repo", "--agent-id", "api", "--reason", "Reset queue."])

  assert.equal(createWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(updateWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(truncateWorkforceMock.mock.calls.length, 1)
  assert.deepEqual(createWorkforceRequestMock.mock.calls[0]?.[0], {
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Ship it.",
  })
  assert.match(String(consoleLog.mock.calls[0]?.[0]), /Queued workforce request req-1\./)
  assert.match(String(consoleLog.mock.calls[1]?.[0]), /Updated workforce request req-1\./)
  assert.match(String(consoleLog.mock.calls[2]?.[0]), /Truncated workforce queue for \/repo\./)
})

test("request accepts -t as a short alias for --target-agent-id", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-2" })
  vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["request", "--root", "/repo", "-t", "api", "--message", "Ship it."])

  assert.deepEqual(createWorkforceRequestMock.mock.calls[0]?.[0], {
    rootDir: "/repo",
    targetAgentId: "api",
    message: "Ship it.",
  })
})

test("request accepts the message positionally and lets --message override it", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-2" })
  vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["request", "--root", "/repo", "Ship it."])
  await main(["request", "--root", "/repo", "--message", "Option wins.", "Positional loses."])

  assert.deepEqual(createWorkforceRequestMock.mock.calls[0]?.[0], {
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Ship it.",
  })
  assert.deepEqual(createWorkforceRequestMock.mock.calls[1]?.[0], {
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Option wins.",
  })
})

test("create routes a create-intent request to the root workforce agent", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  getWorkforceMock.mockResolvedValue({
    config: {
      rootAgentId: "root",
    },
  })
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-create-1" })
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main([
    "create",
    "--root",
    "/repo",
    "--message",
    "Build a worker package for scheduled jobs.",
  ])

  assert.equal(getWorkforceMock.mock.calls.length, 1)
  assert.deepEqual(createWorkforceRequestMock.mock.calls[0]?.[0], {
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Build a worker package for scheduled jobs.",
    intent: "create",
  })
  assert.equal(String(consoleLog.mock.calls[0]?.[0]), "Queued create request req-create-1.")
})

test("create accepts the message positionally and lets --message override it", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  getWorkforceMock.mockResolvedValue({
    config: {
      rootAgentId: "root",
    },
  })
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-create-2" })
  vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["create", "--root", "/repo", "Build a worker package."])
  await main(["create", "--root", "/repo", "--message", "Option wins.", "Positional loses."])

  assert.deepEqual(createWorkforceRequestMock.mock.calls[0]?.[0], {
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Build a worker package.",
    intent: "create",
  })
  assert.deepEqual(createWorkforceRequestMock.mock.calls[1]?.[0], {
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Option wins.",
    intent: "create",
  })
})

test("update accepts the message positionally and lets --message override it", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  updateWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["update", "--root", "/repo", "--request-id", "req-1", "Resume it."])
  await main([
    "update",
    "--root",
    "/repo",
    "--request-id",
    "req-1",
    "--message",
    "Option wins.",
    "Positional loses.",
  ])

  assert.deepEqual(updateWorkforceRequestMock.mock.calls[0]?.[0], {
    rootDir: "/repo",
    requestId: "req-1",
    message: "Resume it.",
  })
  assert.deepEqual(updateWorkforceRequestMock.mock.calls[1]?.[0], {
    rootDir: "/repo",
    requestId: "req-1",
    message: "Option wins.",
  })
})

test("help output includes command and arg descriptions", async () => {
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})
  const processExit = vi.spyOn(process, "exit").mockImplementation(() => undefined as never)

  const { main } = await import("../src/main.ts")
  await main(["--help"])
  await main(["request", "--help"])
  await main(["create", "--help"])
  await main(["update", "--help"])

  const helpOutput = consoleLog.mock.calls.map((call) => String(call[0])).join("\n")
  assert.match(helpOutput, /Manage daemon-owned workforce runtimes and requests/)
  assert.match(helpOutput, /Queue a new workforce request for a target agent/)
  assert.match(helpOutput, /--target-agent-id/)
  assert.match(helpOutput, /-t/)
  assert.match(helpOutput, /defaults to root/)
  assert.match(
    helpOutput,
    /Ask the root agent to scaffold a new project or add packages for a feature/,
  )
  assert.match(helpOutput, /--root/)
  assert.match(helpOutput, /Repository root or any path inside the repository/)
  assert.match(helpOutput, /--message/)
  assert.match(helpOutput, /\[message\]/)
  assert.match(helpOutput, /overrides the positional message/)
  assert.match(
    helpOutput,
    /Feature request that may require creating a new project or new workspace packages/,
  )
  assert.equal(processExit.mock.calls.length, 4)
})
