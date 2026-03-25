import { afterEach, beforeEach, expect, test, vi } from "vitest"

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

vi.mock(
  "@clack/prompts",
  async (importOriginal): Promise<typeof import("@clack/prompts")> => ({
    ...(await importOriginal<typeof import("@clack/prompts")>()),
    cancel: cancelMock,
    intro: introMock,
    isCancel: isCancelMock as unknown as (value: unknown) => value is symbol,
    multiselect: multiselectMock,
    outro: outroMock,
  }),
)

vi.mock(
  "@goddard-ai/sdk/node",
  async (importOriginal): Promise<typeof import("@goddard-ai/sdk/node")> => ({
    ...(await importOriginal<typeof import("@goddard-ai/sdk/node")>()),
    cancelWorkforceRequest: cancelWorkforceRequestMock,
    createWorkforceRequest: createWorkforceRequestMock,
    getWorkforce: getWorkforceMock,
    listWorkforces: listWorkforcesMock,
    startWorkforce: startWorkforceMock,
    stopWorkforce: stopWorkforceMock,
    truncateWorkforce: truncateWorkforceMock,
    updateWorkforceRequest: updateWorkforceRequestMock,
  }),
)

vi.mock("@goddard-ai/daemon/workforce", () => ({
  discoverWorkforceInitCandidates: discoverWorkforceInitCandidatesMock,
  initializeWorkforce: initializeWorkforceMock,
  resolveRepositoryRoot: resolveRepositoryRootMock,
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

  expect(consoleLog.mock.calls[0]?.[0]).toBe("No workforce candidates found under /repo.")
  expect(multiselectMock).not.toHaveBeenCalled()
  expect(initializeWorkforceMock).not.toHaveBeenCalled()
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

  expect(cancelMock).toHaveBeenCalledTimes(1)
  expect(initializeWorkforceMock).not.toHaveBeenCalled()
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

  expect(initializeWorkforceMock.mock.calls[0]?.[0]).toBe("/repo")
  expect(initializeWorkforceMock.mock.calls[0]?.[1]).toEqual(["/repo", "/repo/packages/ui"])
  expect(outroMock).toHaveBeenCalledTimes(1)
})

test("start and stop commands forward lifecycle control to the daemon-backed SDK helpers", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  startWorkforceMock.mockResolvedValue({ rootDir: "/repo" })
  stopWorkforceMock.mockResolvedValue(true)

  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})
  const { main } = await import("../src/main.ts")

  await main(["start", "--root", "/repo/packages/app"])
  await main(["stop", "--root", "/repo/packages/app"])

  expect(startWorkforceMock).toHaveBeenCalledTimes(1)
  expect(stopWorkforceMock).toHaveBeenCalledTimes(1)
  expect(String(consoleLog.mock.calls[0]?.[0])).toMatch(/Started workforce for \/repo\./)
  expect(String(consoleLog.mock.calls[1]?.[0])).toMatch(/Stopped workforce for \/repo\./)
})

test("status and list print daemon-backed workforce state", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  getWorkforceMock.mockResolvedValue({ rootDir: "/repo", activeRequestCount: 0 })
  listWorkforcesMock.mockResolvedValue([{ rootDir: "/repo", activeRequestCount: 0 }])
  const consoleLog = vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["status", "--root", "/repo"])
  await main(["list"])

  expect(getWorkforceMock).toHaveBeenCalledTimes(1)
  expect(listWorkforcesMock).toHaveBeenCalledTimes(1)
  expect(String(consoleLog.mock.calls[0]?.[0])).toMatch(/"rootDir": "\/repo"/)
  expect(String(consoleLog.mock.calls[1]?.[0])).toMatch(/"activeRequestCount": 0/)
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

  expect(createWorkforceRequestMock).toHaveBeenCalledTimes(1)
  expect(updateWorkforceRequestMock).toHaveBeenCalledTimes(1)
  expect(truncateWorkforceMock).toHaveBeenCalledTimes(1)
  expect(createWorkforceRequestMock.mock.calls[0]?.[0]).toEqual({
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Ship it.",
  })
  expect(String(consoleLog.mock.calls[0]?.[0])).toMatch(/Queued workforce request req-1\./)
  expect(String(consoleLog.mock.calls[1]?.[0])).toMatch(/Updated workforce request req-1\./)
  expect(String(consoleLog.mock.calls[2]?.[0])).toMatch(/Truncated workforce queue for \/repo\./)
})

test("request accepts -t as a short alias for --target-agent-id", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  createWorkforceRequestMock.mockResolvedValue({ requestId: "req-2" })
  vi.spyOn(console, "log").mockImplementation(() => {})

  const { main } = await import("../src/main.ts")
  await main(["request", "--root", "/repo", "-t", "api", "--message", "Ship it."])

  expect(createWorkforceRequestMock.mock.calls[0]?.[0]).toEqual({
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

  expect(createWorkforceRequestMock.mock.calls[0]?.[0]).toEqual({
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Ship it.",
  })
  expect(createWorkforceRequestMock.mock.calls[1]?.[0]).toEqual({
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

  expect(getWorkforceMock).toHaveBeenCalledTimes(1)
  expect(createWorkforceRequestMock.mock.calls[0]?.[0]).toEqual({
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Build a worker package for scheduled jobs.",
    intent: "create",
  })
  expect(String(consoleLog.mock.calls[0]?.[0])).toBe("Queued create request req-create-1.")
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

  expect(createWorkforceRequestMock.mock.calls[0]?.[0]).toEqual({
    rootDir: "/repo",
    targetAgentId: "root",
    message: "Build a worker package.",
    intent: "create",
  })
  expect(createWorkforceRequestMock.mock.calls[1]?.[0]).toEqual({
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

  expect(updateWorkforceRequestMock.mock.calls[0]?.[0]).toEqual({
    rootDir: "/repo",
    requestId: "req-1",
    message: "Resume it.",
  })
  expect(updateWorkforceRequestMock.mock.calls[1]?.[0]).toEqual({
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
  expect(helpOutput).toMatch(/Manage daemon-owned workforce runtimes and requests/)
  expect(helpOutput).toMatch(/Queue a new workforce request for a target agent/)
  expect(helpOutput).toMatch(/--target-agent-id/)
  expect(helpOutput).toMatch(/-t/)
  expect(helpOutput).toMatch(/defaults to root/)
  expect(helpOutput).toMatch(
    /Ask the root agent to scaffold a new project or add packages for a feature/,
  )
  expect(helpOutput).toMatch(/--root/)
  expect(helpOutput).toMatch(/Repository root or any path inside the repository/)
  expect(helpOutput).toMatch(/--message/)
  expect(helpOutput).toMatch(/\[message\]/)
  expect(helpOutput).toMatch(/overrides the positional message/)
  expect(helpOutput).toMatch(
    /Feature request that may require creating a new project or new workspace packages/,
  )
  expect(processExit).toHaveBeenCalledTimes(4)
})
