import { spawnSync } from "node:child_process"
import { lstat, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { createServer, type ServerResponse } from "node:http"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import { resolveRuntimeConfig } from "../src/config.ts"
import { runDaemon } from "../src/daemon.ts"
import {
  createDaemonUrl,
  readDaemonTcpAddressFromDaemonUrl,
  resolveReplyRequestFromGit,
  resolveSubmitRequestFromGit,
} from "../src/ipc.ts"
import { db, resetDb } from "../src/persistence/store.ts"
import { createWrappedNodeAgent } from "./acp-fixture.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const agentBinDir = fileURLToPath(new URL("../agent-bin", import.meta.url))
const fastFixtureAgentPath = fileURLToPath(
  new URL("./fixtures/fast-acp-agent.mjs", import.meta.url),
)
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"

afterEach(async () => {
  resetDb({ filename: ":memory:" })

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("daemon package ships agent-bin wrappers for goddard and workforce", async () => {
  const wrapperPath = new URL("../agent-bin/goddard", import.meta.url)
  const workforceWrapperPath = new URL("../agent-bin/workforce", import.meta.url)
  const [goddardStat, workforceStat] = await Promise.all([
    lstat(wrapperPath),
    lstat(workforceWrapperPath),
  ])
  expect(goddardStat.isSymbolicLink() || goddardStat.isFile()).toBe(true)
  expect(workforceStat.isSymbolicLink() || workforceStat.isFile()).toBe(true)
})

test(
  "daemon run subscribes once and launches managed PR feedback sessions across repositories",
  async () => {
    await useTempHome()
    await writeGlobalRootConfig({
      session: {
        agent: createWrappedNodeAgent(fastFixtureAgentPath),
      },
    })

    const backend = await startBackendHarness()
    cleanup.push(() => backend.close())

    db.metadata.set("authToken", "tok")

    const firstRepoDir = await createRepoFixture()
    const secondRepoDir = await createRepoFixture()
    seedPullRequest({
      owner: "other",
      repo: "repo",
      prNumber: 123,
      cwd: firstRepoDir,
    })
    seedPullRequest({
      owner: "test",
      repo: "repo",
      prNumber: 123,
      cwd: secondRepoDir,
    })

    const port = await getUnusedTcpPort()

    const { logs, result: exitCode } = await captureJsonLogs(async (output) => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        logMode: "json",
      })
      const stopDaemon = createDaemonStopper()

      try {
        await waitFor(async () => {
          const healthy = await isDaemonHealthy(port)
          return healthy && backend.subscriptionCount() === 1
        })

        backend.sendEvent({
          type: "comment",
          owner: "other",
          repo: "repo",
          prNumber: 123,
          author: "alice",
          body: "handle this too",
          reactionAdded: "eyes",
          createdAt: new Date().toISOString(),
        })

        await waitFor(
          () => {
            const sessions = db.sessions.findMany()
            return (
              sessions.length === 1 &&
              parseJsonLogs(output).filter((entry) => entry.event === "pr_feedback.finish")
                .length === 1
            )
          },
          { timeoutMs: 15000 },
        )

        backend.sendEvent({
          type: "comment",
          owner: "test",
          repo: "repo",
          prNumber: 123,
          author: "alice",
          body: "fix it",
          reactionAdded: "eyes",
          createdAt: new Date().toISOString(),
        })

        await waitFor(
          () => {
            const sessions = db.sessions.findMany()
            return (
              sessions.length === 2 &&
              parseJsonLogs(output).filter((entry) => entry.event === "pr_feedback.finish")
                .length === 2
            )
          },
          { timeoutMs: 15000 },
        )

        await stopDaemon()
        return await daemonPromise
      } finally {
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(1)
    expect(
      db.sessions
        .findMany()
        .map(({ repository, prNumber, stopReason }) => ({
          repository,
          prNumber,
          stopReason,
        }))
        .sort((left, right) => (left.repository ?? "").localeCompare(right.repository ?? "")),
    ).toEqual([
      {
        repository: "other/repo",
        prNumber: 123,
        stopReason: "end_turn",
      },
      {
        repository: "test/repo",
        prNumber: 123,
        stopReason: "end_turn",
      },
    ])

    const startupLog = logs.find((entry) => entry.event === "daemon.startup")
    expect(startupLog).toEqual({
      scope: "daemon",
      at: startupLog?.at,
      event: "daemon.startup",
      baseUrl: backend.baseUrl,
      port,
      agentBinDir,
    })
    expect(logs.some((entry) => entry.event === "repo.subscription_started")).toBe(true)
    expect(
      logs
        .filter((entry) => entry.event === "pr_feedback.launch")
        .map((entry) => {
          const feedbackEvent = entry.feedbackEvent as Record<string, unknown>
          return `${feedbackEvent.repository}#${feedbackEvent.prNumber}`
        })
        .sort(),
    ).toEqual(["other/repo#123", "test/repo#123"])
    expect(logs.some((entry) => entry.event === "pr_feedback.session_create_failed")).toBe(false)
    expect(
      logs.filter((entry) => entry.event === "pr_feedback.finish").map((entry) => entry.exitCode),
    ).toEqual([0, 0])
    expect(logs.some((entry) => entry.event === "daemon.shutdown")).toBe(true)
  },
  { timeout: 20000 },
)

test(
  "daemon run can start only the IPC server when stream is disabled",
  async () => {
    await useTempHome()
    const backend = await startBackendHarness()
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")

    const port = await getUnusedTcpPort()

    const { logs, result: exitCode } = await captureJsonLogs(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        enableIpc: true,
        enableStream: false,
        logMode: "json",
      })
      const stopDaemon = createDaemonStopper()

      try {
        await waitFor(async () => {
          return isDaemonHealthy(port)
        })
        await stopDaemon()
        return await daemonPromise
      } finally {
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(0)
    expect(logs.some((entry) => entry.event === "repo.subscription_started")).toBe(false)
    expect(logs.some((entry) => entry.event === "ipc.server_listening")).toBe(true)
    expect(logs.some((entry) => entry.event === "daemon.shutdown")).toBe(true)
  },
  { timeout: 10000 },
)

test(
  "daemon run can subscribe without IPC and ignores feedback that requires the PR feedback flow",
  async () => {
    await useTempHome()
    const backend = await startBackendHarness()
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")

    const { logs, result: exitCode } = await captureJsonLogs(async (output) => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        enableIpc: false,
        enableStream: true,
        logMode: "json",
      })
      const stopDaemon = createDaemonStopper()

      try {
        await waitFor(() => backend.subscriptionCount() === 1)
        backend.sendEvent({
          type: "comment",
          owner: "test",
          repo: "repo",
          prNumber: 456,
          author: "alice",
          body: "fix it",
          reactionAdded: "eyes",
          createdAt: new Date().toISOString(),
        })
        await waitFor(() =>
          parseJsonLogs(output).some(
            (entry) => entry.event === "repo.feedback_ignored" && entry.reason === "ipc_disabled",
          ),
        )
        await stopDaemon()
        return await daemonPromise
      } finally {
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(1)
    expect(db.sessions.findMany()).toHaveLength(0)
    expect(
      logs.some(
        (entry) => entry.event === "repo.feedback_ignored" && entry.reason === "ipc_disabled",
      ),
    ).toBe(true)
  },
  { timeout: 10000 },
)

test(
  "daemon run keeps IPC available when stream startup is unauthenticated",
  async () => {
    await useTempHome()
    const backend = await startBackendHarness({
      rejectStreamUnauthorized: true,
    })
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")

    const port = await getUnusedTcpPort()

    const { logs, result: exitCode } = await captureJsonLogs(async (output) => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        logMode: "json",
      })
      const stopDaemon = createDaemonStopper()

      try {
        await waitFor(async () => {
          const healthy = await isDaemonHealthy(port)
          return (
            healthy &&
            parseJsonLogs(output).some(
              (entry) =>
                entry.event === "repo.subscription_degraded" && entry.reason === "unauthenticated",
            )
          )
        })
        await stopDaemon()
        return await daemonPromise
      } finally {
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(0)
    expect(
      logs.some(
        (entry) =>
          entry.event === "repo.subscription_degraded" && entry.reason === "unauthenticated",
      ),
    ).toBe(true)
    expect(logs.some((entry) => entry.event === "repo.subscription_started")).toBe(false)
    expect(logs.some((entry) => entry.event === "daemon.run_failed")).toBe(false)
    expect(logs.some((entry) => entry.event === "daemon.shutdown")).toBe(true)
  },
  { timeout: 10000 },
)

test("daemon run defaults to concise pretty terminal logs", async () => {
  const { output, result: exitCode } = await captureStdout(() =>
    runDaemon({
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
    }),
  )

  expect(exitCode).toBe(0)
  expect(output.some((line) => line.includes("daemon.startup"))).toBe(true)
  expect(output.some((line) => line.includes("daemon.no_features_enabled"))).toBe(true)
  expect(output.every((line) => line.trim().startsWith("{"))).toBe(false)
})

test("daemon run supports raw json terminal logs when requested", async () => {
  const { output, result: exitCode } = await captureStdout(() =>
    runDaemon({
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
      logMode: "json",
    }),
  )

  expect(exitCode).toBe(0)
  expect(output.some((line) => line.includes('"event":"daemon.startup"'))).toBe(true)
  expect(output.some((line) => line.includes('"event":"daemon.no_features_enabled"'))).toBe(true)
  expect(output.every((line) => line.trim().startsWith("{"))).toBe(true)
})

test("daemon run supports verbose terminal logs with expanded fields", async () => {
  const { output, result: exitCode } = await captureStdout(() =>
    runDaemon({
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
      logMode: "verbose",
    }),
  )

  expect(exitCode).toBe(0)
  expect(output.some((line) => line.includes("daemon.startup"))).toBe(true)
  expect(output.some((line) => line.includes("baseUrl:"))).toBe(true)
  expect(output.every((line) => line.trim().startsWith("{"))).toBe(false)
})

test("daemon URL round-trips the TCP address", () => {
  const daemonUrl = createDaemonUrl(49827)

  expect(daemonUrl).toBe("http://127.0.0.1:49827/")
  expect(readDaemonTcpAddressFromDaemonUrl(daemonUrl)).toEqual({
    hostname: "127.0.0.1",
    port: 49827,
  })
})

test("daemon runtime resolves the global daemon port override", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    daemon: {
      port: 41236,
    },
  })

  expect(resolveRuntimeConfig().port).toBe(41236)
})

test("daemon resolves PR context from git metadata", async () => {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-git-"))
  cleanup.push(async () => {
    await rm(repoDir, { recursive: true, force: true })
  })

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.name", "Goddard"])
  runGit(repoDir, ["config", "user.email", "goddard@example.com"])
  await writeFile(join(repoDir, "README.md"), "# test\n", "utf-8")
  runGit(repoDir, ["add", "README.md"])
  runGit(repoDir, ["commit", "-m", "init"])
  runGit(repoDir, ["checkout", "-b", "feature/ipc"])
  runGit(repoDir, ["remote", "add", "origin", "git@github.com:acme/widgets.git"])
  await mkdir(join(repoDir, ".git", "refs", "remotes", "origin"), {
    recursive: true,
  })
  await writeFile(
    join(repoDir, ".git", "refs", "remotes", "origin", "HEAD"),
    "ref: refs/remotes/origin/main\n",
  )

  const submit = await resolveSubmitRequestFromGit({
    cwd: repoDir,
    title: "Implement IPC routing",
    body: "Done.",
  })
  expect(submit).toEqual({
    owner: "acme",
    repo: "widgets",
    title: "Implement IPC routing",
    body: "Done.",
    head: "feature/ipc",
    base: "main",
  })

  runGit(repoDir, ["checkout", "-B", "pr-12"])
  const reply = await resolveReplyRequestFromGit({
    cwd: repoDir,
    message: "Updated per review",
  })
  expect(reply).toEqual({
    owner: "acme",
    repo: "widgets",
    prNumber: 12,
    body: "Updated per review",
  })
})

function runGit(cwd: string, args: string[]): void {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })
  expect(result.status).toBe(0)
}

async function captureJsonLogs<T>(
  action: (output: string[]) => Promise<T>,
): Promise<{ logs: Array<Record<string, unknown>>; result: T }> {
  const { output, result } = await captureStdout(action)
  return {
    logs: parseJsonLogs(output),
    result,
  }
}

async function captureStdout<T>(
  action: (output: string[]) => Promise<T>,
): Promise<{ output: string[]; result: T }> {
  const output: string[] = []
  const originalWrite = process.stdout.write.bind(process.stdout)
  process.stdout.write = ((chunk: string | Uint8Array, ...rest: unknown[]) => {
    output.push(typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf-8"))
    const callback = rest.find((value) => typeof value === "function")
    if (typeof callback === "function") {
      callback()
    }
    return true
  }) as typeof process.stdout.write

  try {
    const result = await action(output)
    return { output, result }
  } finally {
    process.stdout.write = originalWrite
  }
}

function parseJsonLogs(output: string[]): Array<Record<string, unknown>> {
  return output
    .flatMap((chunk) => chunk.split("\n"))
    .filter((line) => line.trim().length > 0)
    .map((line) => JSON.parse(line) as Record<string, unknown>)
}

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-daemon-home-"))
  cleanup.push(async () => {
    await rm(homeDir, { recursive: true, force: true })
  })
  process.env.HOME = homeDir
  resetDb({ filename: ":memory:" })
  return homeDir
}

async function writeGlobalRootConfig(config: Record<string, unknown>) {
  const configPath = getGlobalConfigPath()
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({ $schema: rootConfigSchemaUrl, ...config }, null, 2)}\n`,
    "utf-8",
  )
}

async function createRepoFixture(): Promise<string> {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-run-repo-"))
  cleanup.push(async () => {
    await rm(repoDir, { recursive: true, force: true })
  })

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "repo", private: true }, null, 2),
    "utf-8",
  )

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "."])
  runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

function seedPullRequest(input: { owner: string; repo: string; prNumber: number; cwd: string }) {
  db.pullRequests.create({
    host: "github",
    owner: input.owner,
    repo: input.repo,
    prNumber: input.prNumber,
    cwd: input.cwd,
  })
}

async function startBackendHarness(
  options: {
    rejectStreamUnauthorized?: boolean
    isManaged?: (input: { owner: string; repo: string; prNumber: number }) => boolean
  } = {},
) {
  const streams = new Set<ServerResponse>()
  let subscriptionCount = 0
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "127.0.0.1"}`)

    if (url.pathname === "/stream") {
      if (options.rejectStreamUnauthorized || !request.headers.authorization) {
        response.writeHead(401, { "content-type": "text/plain" })
        response.end("unauthorized")
        return
      }

      subscriptionCount += 1
      response.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
      })
      response.write(": connected\n\n")
      streams.add(response)
      request.on("close", () => {
        streams.delete(response)
      })
      return
    }

    if (url.pathname === "/pr/managed") {
      if (!request.headers.authorization) {
        response.writeHead(401, { "content-type": "text/plain" })
        response.end("unauthorized")
        return
      }

      const owner = url.searchParams.get("owner") ?? ""
      const repo = url.searchParams.get("repo") ?? ""
      const prNumber = Number(url.searchParams.get("prNumber") ?? "0")
      const managed = options.isManaged?.({ owner, repo, prNumber }) ?? true
      response.writeHead(200, { "content-type": "application/json" })
      response.end(JSON.stringify({ managed }))
      return
    }

    response.writeHead(404, { "content-type": "text/plain" })
    response.end("not found")
  })

  await new Promise<void>((resolve, reject) => {
    const onError = (error: Error) => {
      server.off("listening", onListening)
      reject(error)
    }
    const onListening = () => {
      server.off("error", onError)
      resolve()
    }

    server.once("error", onError)
    server.once("listening", onListening)
    server.listen(0, "127.0.0.1")
  })

  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("Backend harness did not bind to a TCP port")
  }

  return {
    baseUrl: `http://127.0.0.1:${address.port}`,
    subscriptionCount() {
      return subscriptionCount
    },
    sendEvent(event: unknown) {
      const frame = `data: ${JSON.stringify({ event })}\n\n`
      for (const stream of [...streams]) {
        stream.write(frame)
      }
    },
    async close() {
      for (const stream of streams) {
        stream.end()
      }

      await new Promise<void>((resolve, reject) => {
        server.close((error) => {
          if (error) {
            reject(error)
            return
          }

          resolve()
        })
      })
    },
  }
}

async function getUnusedTcpPort() {
  const server = createServer((_request, response) => {
    response.writeHead(204)
    response.end()
  })

  await new Promise<void>((resolve, reject) => {
    const onError = (error: Error) => {
      server.off("listening", onListening)
      reject(error)
    }
    const onListening = () => {
      server.off("error", onError)
      resolve()
    }

    server.once("error", onError)
    server.once("listening", onListening)
    server.listen(0, "127.0.0.1")
  })

  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("TCP port probe did not bind to a TCP port")
  }

  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error)
        return
      }

      resolve()
    })
  })

  return address.port
}

async function isDaemonHealthy(port: number) {
  try {
    const client = createDaemonIpcClient({
      daemonUrl: createDaemonUrl(port),
    })
    const response = await client.send("daemon.health")
    return response.ok === true
  } catch {
    return false
  }
}

async function emitSigint() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  process.emit("SIGINT")
}

function createDaemonStopper() {
  let stopped = false

  return async () => {
    if (stopped) {
      return
    }

    stopped = true
    await emitSigint()
  }
}

async function waitFor<T>(
  condition: () => Promise<T> | T,
  options: { timeoutMs?: number; intervalMs?: number } = {},
): Promise<T> {
  const timeoutMs = options.timeoutMs ?? 5000
  const intervalMs = options.intervalMs ?? 25
  const deadline = Date.now() + timeoutMs

  while (true) {
    const result = await condition()
    if (result) {
      return result
    }

    if (Date.now() >= deadline) {
      throw new Error("Timed out waiting for test condition")
    }

    await new Promise((resolve) => setTimeout(resolve, intervalMs))
  }
}
