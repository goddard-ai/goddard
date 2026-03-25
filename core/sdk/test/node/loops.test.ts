import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, beforeEach, expect, test, vi } from "vitest"
import { GoddardSdk } from "../../src/node/index.ts"
import { buildLoopStartRequest, resolveLoop, startNamedLoop } from "../../src/node/loops.ts"

const mockedDaemonLoops = vi.hoisted(() => ({
  getDaemonLoop: vi.fn(),
  listDaemonLoops: vi.fn(),
  shutdownDaemonLoop: vi.fn(),
  startDaemonLoop: vi.fn(),
}))

vi.mock(
  "../../src/daemon/loops.ts",
  async (importOriginal): Promise<typeof import("../../src/daemon/loops.ts")> => ({
    ...(await importOriginal<typeof import("../../src/daemon/loops.ts")>()),
    ...mockedDaemonLoops,
  }),
)

vi.mock("@goddard-ai/config", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@goddard-ai/config")>()
  return {
    ...actual,
    resolveDefaultAgent: vi.fn().mockResolvedValue("pi-acp"),
  }
})

const originalHome = process.env.HOME
let isolatedHomeDir: string | null = null

beforeEach(async () => {
  isolatedHomeDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-sdk-home-"))
  process.env.HOME = isolatedHomeDir
})

afterEach(async () => {
  mockedDaemonLoops.getDaemonLoop.mockReset()
  mockedDaemonLoops.listDaemonLoops.mockReset()
  mockedDaemonLoops.shutdownDaemonLoop.mockReset()
  mockedDaemonLoops.startDaemonLoop.mockReset()

  if (isolatedHomeDir) {
    await fs.rm(isolatedHomeDir, { recursive: true, force: true })
    isolatedHomeDir = null
  }

  if (originalHome === undefined) {
    delete process.env.HOME
    return
  }

  process.env.HOME = originalHome
})

test("resolveLoop merges root defaults with packaged loop config", async () => {
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-dir-"))
  const loopDir = path.join(tempDir, ".goddard", "loops", "review")

  await fs.mkdir(loopDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      loops: {
        session: {
          agent: "pi-acp",
          mcpServers: [],
        },
        rateLimits: {
          cycleDelay: "1s",
          maxOpsPerMinute: 5,
          maxCyclesBeforePause: 10,
        },
      },
    }),
    "utf-8",
  )
  await fs.writeFile(
    path.join(loopDir, "config.json"),
    JSON.stringify({
      retries: {
        maxAttempts: 2,
      },
    }),
    "utf-8",
  )
  await fs.writeFile(
    path.join(loopDir, "prompt.js"),
    'export function nextPrompt() { return "Continue the current task." }\n',
    "utf-8",
  )

  const loop = await resolveLoop("review", tempDir)

  expect(loop.promptModulePath).toBe(path.join(loopDir, "prompt.js"))
  expect(loop.config).toEqual({
    session: {
      agent: "pi-acp",
      mcpServers: [],
    },
    rateLimits: {
      cycleDelay: "1s",
      maxOpsPerMinute: 5,
      maxCyclesBeforePause: 10,
    },
    retries: {
      maxAttempts: 2,
    },
  })
})

test("buildLoopStartRequest resolves packaged config into daemon payload", () => {
  const request = buildLoopStartRequest(
    "review",
    "/repo",
    {
      path: "/repo/.goddard/loops/review",
      promptModulePath: "/repo/.goddard/loops/review/prompt.js",
      config: {
        session: {
          agent: "pi-acp",
          mcpServers: [],
        },
        rateLimits: {
          cycleDelay: "1s",
          maxOpsPerMinute: 5,
          maxCyclesBeforePause: 10,
        },
      },
    },
    {
      session: {
        systemPrompt: "Use the loop checklist.",
      },
      retries: {
        maxAttempts: 3,
      },
    },
  )

  expect(request).toEqual({
    rootDir: path.resolve("/repo"),
    loopName: "review",
    promptModulePath: path.resolve("/repo/.goddard/loops/review/prompt.js"),
    session: {
      agent: "pi-acp",
      cwd: "/repo",
      mcpServers: [],
      systemPrompt: "Use the loop checklist.",
    },
    rateLimits: {
      cycleDelay: "1s",
      maxOpsPerMinute: 5,
      maxCyclesBeforePause: 10,
    },
    retries: {
      maxAttempts: 3,
      initialDelayMs: 500,
      maxDelayMs: 5_000,
      backoffFactor: 2,
      jitterRatio: 0.2,
    },
  })
})

test("resolveLoop rejects prompt.md-only loop packages", async () => {
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-invalid-"))
  const loopDir = path.join(tempDir, ".goddard", "loops", "review")

  await fs.mkdir(loopDir, { recursive: true })
  await fs.writeFile(path.join(loopDir, "prompt.md"), "Legacy prompt.\n", "utf-8")
  await fs.writeFile(path.join(loopDir, "config.json"), JSON.stringify({}), "utf-8")

  await expect(resolveLoop("review", tempDir)).rejects.toThrow(/must not contain prompt\.md/)
})

test("startNamedLoop forwards the resolved daemon start payload", async () => {
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-start-"))
  const loopDir = path.join(tempDir, ".goddard", "loops", "review")

  mockedDaemonLoops.startDaemonLoop.mockResolvedValue({
    rootDir: tempDir,
    loopName: "review",
  })

  await fs.mkdir(loopDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      loops: {
        session: {
          agent: "pi-acp",
          mcpServers: [],
        },
        rateLimits: {
          cycleDelay: "30s",
          maxOpsPerMinute: 4,
          maxCyclesBeforePause: 200,
        },
      },
    }),
    "utf-8",
  )
  await fs.writeFile(path.join(loopDir, "config.json"), JSON.stringify({}), "utf-8")
  await fs.writeFile(
    path.join(loopDir, "prompt.js"),
    'export function nextPrompt() { return "Continue the current task." }\n',
    "utf-8",
  )

  await expect(startNamedLoop("review", { session: { cwd: tempDir } })).resolves.toEqual({
    rootDir: tempDir,
    loopName: "review",
  })

  expect(mockedDaemonLoops.startDaemonLoop).toHaveBeenCalledWith(
    {
      rootDir: tempDir,
      loopName: "review",
      promptModulePath: path.join(loopDir, "prompt.js"),
      session: {
        agent: "pi-acp",
        cwd: tempDir,
        mcpServers: [],
      },
      rateLimits: {
        cycleDelay: "30s",
        maxOpsPerMinute: 4,
        maxCyclesBeforePause: 200,
      },
      retries: {
        maxAttempts: 1,
        initialDelayMs: 500,
        maxDelayMs: 5_000,
        backoffFactor: 2,
        jitterRatio: 0.2,
      },
    },
    undefined,
  )
})

test("node SDK loop helpers no longer expose ad hoc run entrypoints", async () => {
  const sdk = new GoddardSdk({ backendUrl: "https://api.example.com" })

  expect("run" in sdk.loop).toBe(false)
  expect("runNamed" in sdk.loop).toBe(false)
  expect(Object.keys(sdk.loop).sort()).toEqual(["get", "list", "resolve", "start", "stop"])
})
