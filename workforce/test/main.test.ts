import * as assert from "node:assert/strict"
import { afterEach, beforeEach, test, vi } from "vitest"

const cancelledSelection = Symbol("cancelled-selection")

const {
  cancelMock,
  discoverWorkforceInitCandidatesMock,
  initializeWorkforcePackagesMock,
  introMock,
  isCancelMock,
  multiselectMock,
  outroMock,
  resolveRepositoryRootMock,
  stopMock,
  watchWorkforceMock,
} = vi.hoisted(() => ({
  cancelMock: vi.fn(),
  discoverWorkforceInitCandidatesMock: vi.fn(),
  initializeWorkforcePackagesMock: vi.fn(),
  introMock: vi.fn(),
  isCancelMock: vi.fn((value: unknown) => value === cancelledSelection),
  multiselectMock: vi.fn(),
  outroMock: vi.fn(),
  resolveRepositoryRootMock: vi.fn(),
  stopMock: vi.fn(async () => {}),
  watchWorkforceMock: vi.fn(async () => ({ stop: stopMock })),
}))

vi.mock("@clack/prompts", () => ({
  cancel: cancelMock,
  intro: introMock,
  isCancel: isCancelMock,
  multiselect: multiselectMock,
  outro: outroMock,
}))

vi.mock("@goddard-ai/sdk/node", () => ({
  discoverWorkforceInitCandidates: discoverWorkforceInitCandidatesMock,
  initializeWorkforcePackages: initializeWorkforcePackagesMock,
  resolveRepositoryRoot: resolveRepositoryRootMock,
  watchWorkforce: watchWorkforceMock,
}))

beforeEach(() => {
  cancelMock.mockReset()
  discoverWorkforceInitCandidatesMock.mockReset()
  initializeWorkforcePackagesMock.mockReset()
  introMock.mockReset()
  isCancelMock.mockClear()
  multiselectMock.mockReset()
  outroMock.mockReset()
  resolveRepositoryRootMock.mockReset()
  stopMock.mockClear()
  watchWorkforceMock.mockReset()
  watchWorkforceMock.mockResolvedValue({ stop: stopMock })
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
  assert.equal(initializeWorkforcePackagesMock.mock.calls.length, 0)
})

test("init cancels without creating files when the prompt is cancelled", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  discoverWorkforceInitCandidatesMock.mockResolvedValue([
    {
      rootDir: "/repo",
      relativeDir: ".",
      manifestPath: "/repo/package.json",
      name: "@repo/root",
      goddardDir: "/repo/.goddard",
      requestsPath: "/repo/.goddard/requests.jsonl",
      responsesPath: "/repo/.goddard/responses.jsonl",
    },
  ])
  multiselectMock.mockResolvedValue(cancelledSelection)

  const { main } = await import("../src/main.ts")
  await main(["init", "--root", "/repo"])

  assert.equal(cancelMock.mock.calls.length, 1)
  assert.equal(initializeWorkforcePackagesMock.mock.calls.length, 0)
})

test("init maps selected package roots into workforce initialization", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")
  discoverWorkforceInitCandidatesMock.mockResolvedValue([
    {
      rootDir: "/repo",
      relativeDir: ".",
      manifestPath: "/repo/package.json",
      name: "@repo/root",
      goddardDir: "/repo/.goddard",
      requestsPath: "/repo/.goddard/requests.jsonl",
      responsesPath: "/repo/.goddard/responses.jsonl",
    },
    {
      rootDir: "/repo/packages/ui",
      relativeDir: "packages/ui",
      manifestPath: "/repo/packages/ui/package.json",
      name: "@repo/ui",
      goddardDir: "/repo/packages/ui/.goddard",
      requestsPath: "/repo/packages/ui/.goddard/requests.jsonl",
      responsesPath: "/repo/packages/ui/.goddard/responses.jsonl",
    },
  ])
  multiselectMock.mockResolvedValue(["/repo", "/repo/packages/ui"])
  initializeWorkforcePackagesMock.mockResolvedValue([
    {
      packageDir: "/repo",
      goddardDir: "/repo/.goddard",
      createdPaths: ["/repo/.goddard/requests.jsonl"],
    },
    {
      packageDir: "/repo/packages/ui",
      goddardDir: "/repo/packages/ui/.goddard",
      createdPaths: ["/repo/packages/ui/.goddard/requests.jsonl"],
    },
  ])

  const { main } = await import("../src/main.ts")
  await main(["init", "--root", "/repo"])

  assert.deepEqual(initializeWorkforcePackagesMock.mock.calls[0][0], ["/repo", "/repo/packages/ui"])
  assert.equal(outroMock.mock.calls.length, 1)
})

test("watch parses args and forwards daemon options into the SDK watcher", async () => {
  resolveRepositoryRootMock.mockResolvedValue("/repo")

  const { main } = await import("../src/main.ts")
  const runPromise = main([
    "watch",
    "--root",
    "/repo/packages/app",
    "--daemon-url",
    "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
  ])

  setTimeout(() => {
    process.emit("SIGINT")
  }, 20)

  await runPromise

  const firstWatchCall = watchWorkforceMock.mock.calls[0] as unknown as [unknown] | undefined
  assert.ok(firstWatchCall)
  assert.equal((firstWatchCall[0] as { rootDir: string }).rootDir, "/repo")
  assert.deepEqual((firstWatchCall[0] as { daemon?: { daemonUrl: string } }).daemon, {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
  })
  assert.equal(typeof (firstWatchCall[0] as { onEvent: unknown }).onEvent, "function")
  assert.equal(stopMock.mock.calls.length, 1)
})
