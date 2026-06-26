import { lstat, mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { createServer, type ServerResponse } from "node:http"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import { resolveRuntimeConfig } from "../src/config.ts"
import { runDaemon } from "../src/daemon.ts"
import { createDaemonUrl, readDaemonTcpAddressFromDaemonUrl } from "../src/ipc.ts"
import { createWrappedNodeAgent } from "./acp-fixture.ts"
import { resetComposedDaemonStore, type ComposedDaemonStore } from "./support/store.ts"
import { removeTemporaryPath } from "./support/temp.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const originalDaemonPort = process.env.GODDARD_DAEMON_PORT
const originalBaseUrl = process.env.GODDARD_BASE_URL
const agentBinDir = fileURLToPath(new URL("../agent-bin", import.meta.url))
const fastFixtureAgentPath = fileURLToPath(
  new URL("./fixtures/fast-acp-agent.mjs", import.meta.url),
)
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"
let db: ComposedDaemonStore = resetComposedDaemonStore({ filename: ":memory:" })

afterEach(async () => {
  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
  if (originalDaemonPort === undefined) {
    delete process.env.GODDARD_DAEMON_PORT
  } else {
    process.env.GODDARD_DAEMON_PORT = originalDaemonPort
  }
  if (originalBaseUrl === undefined) {
    delete process.env.GODDARD_BASE_URL
  } else {
    process.env.GODDARD_BASE_URL = originalBaseUrl
  }

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  db.close()
  db = resetComposedDaemonStore({ filename: ":memory:" })
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
    const feedbackFinishedEvents: Array<{
      name?: string
      payload?: {
        repository?: string
        prNumber?: number
        feedbackType?: string
        exitCode?: number
      }
    }> = []
    let sessionSummaries: Array<{
      repository: string | null
      prNumber: number | null
      stopReason: string | null
    }> = []

    const { result: exitCode } = await captureStdout(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        logMode: "json",
        store: db,
      })
      const stopDaemon = createDaemonStopper()
      let unsubscribeEvents: (() => Promise<void>) | undefined

      try {
        await waitFor(async () => {
          const healthy = await isDaemonHealthy(port)
          return healthy && backend.subscriptionCount() === 1
        })
        const client = createDaemonIpcClient({
          daemonUrl: createDaemonUrl(port),
        })
        const abortController = new AbortController()
        const eventStream = await client.events.stream(
          {
            names: ["pull_request.feedback.finished"],
          },
          {
            signal: abortController.signal,
          },
        )
        const eventsDone = (async () => {
          for await (const event of eventStream) {
            feedbackFinishedEvents.push(event as (typeof feedbackFinishedEvents)[number])
          }
        })()
        unsubscribeEvents = async () => {
          abortController.abort()
          await eventsDone.catch(() => {})
        }
        backend.sendEvent({
          type: "comment",
          provider: "github",
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
              feedbackFinishedEvents.some(
                (event) =>
                  event.name === "pull_request.feedback.finished" &&
                  event.payload?.repository === "other/repo" &&
                  event.payload?.prNumber === 123 &&
                  event.payload?.feedbackType === "comment" &&
                  event.payload?.exitCode === 0,
              )
            )
          },
          { timeoutMs: 15000 },
        )

        backend.sendEvent({
          type: "comment",
          provider: "github",
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
            return sessions.length === 2 && feedbackFinishedEvents.length === 2
          },
          { timeoutMs: 15000 },
        )
        sessionSummaries = db.sessions
          .findMany()
          .map(({ repository, prNumber, stopReason }) => ({
            repository,
            prNumber,
            stopReason,
          }))
          .sort((left, right) => (left.repository ?? "").localeCompare(right.repository ?? ""))

        await stopDaemon()
        return await daemonPromise
      } finally {
        await unsubscribeEvents?.()
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(1)
    expect(sessionSummaries).toEqual([
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
    expect(
      feedbackFinishedEvents
        .map(
          (event) =>
            `${event.payload?.repository}#${event.payload?.prNumber}:${event.payload?.exitCode}`,
        )
        .sort(),
    ).toEqual(["other/repo#123:0", "test/repo#123:0"])
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

    const { result: exitCode } = await captureStdout(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        enableIpc: true,
        enableStream: false,
        logMode: "json",
        store: db,
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
  },
  { timeout: 10000 },
)

test(
  "daemon run emits backend stream started when the subscription opens",
  async () => {
    await useTempHome()
    const streamResponse = createDeferred<void>()
    const backend = await startBackendHarness({
      beforeStreamResponse: () => streamResponse.promise,
    })
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")

    const port = await getUnusedTcpPort()
    const startedEvents: Array<{
      name?: string
      payload?: {
        daemonUrl?: string
        port?: number
      }
    }> = []

    const { result: exitCode } = await captureStdout(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        logMode: "json",
        store: db,
      })
      const stopDaemon = createDaemonStopper()
      let unsubscribeEvents: (() => Promise<void>) | undefined

      try {
        await waitFor(async () => isDaemonHealthy(port))
        const client = createDaemonIpcClient({
          daemonUrl: createDaemonUrl(port),
        })
        const abortController = new AbortController()
        const eventStream = await client.events.stream(
          {
            names: ["backend.stream.started"],
          },
          {
            signal: abortController.signal,
          },
        )
        const eventsDone = (async () => {
          for await (const event of eventStream) {
            startedEvents.push(event as (typeof startedEvents)[number])
          }
        })()
        unsubscribeEvents = async () => {
          abortController.abort()
          await eventsDone.catch(() => {})
        }

        streamResponse.resolve()
        await waitFor(() =>
          startedEvents.some(
            (event) =>
              event.name === "backend.stream.started" &&
              event.payload?.daemonUrl === createDaemonUrl(port) &&
              event.payload?.port === port,
          ),
        )
        await stopDaemon()
        return await daemonPromise
      } finally {
        await unsubscribeEvents?.()
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(1)
  },
  { timeout: 10000 },
)

test(
  "daemon run skips backend stream without IPC-owned backend event handlers",
  async () => {
    await useTempHome()
    const backend = await startBackendHarness()
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")
    let sessionCount = -1

    const { result: exitCode } = await captureStdout(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        enableIpc: false,
        enableStream: true,
        logMode: "json",
        store: db,
      })
      const stopDaemon = createDaemonStopper()

      try {
        await new Promise((resolve) => setTimeout(resolve, 100))
        sessionCount = db.sessions.findMany().length
        await stopDaemon()
        return await daemonPromise
      } finally {
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })
    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(0)
    expect(sessionCount).toBe(0)
  },
  { timeout: 10000 },
)

test(
  "daemon run emits backend stream degradation when subscription startup fails",
  async () => {
    await useTempHome()
    const streamResponse = createDeferred<void>()
    const backend = await startBackendHarness({
      beforeStreamResponse: () => streamResponse.promise,
      rejectStreamStatus: 503,
    })
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")

    const port = await getUnusedTcpPort()

    const { result: exitCode } = await captureStdout(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        logMode: "json",
        store: db,
      })
      const stopDaemon = createDaemonStopper()
      let unsubscribeEvents: (() => Promise<void>) | undefined
      const degradedEvents: Array<{
        name?: string
        payload?: {
          reason?: string
          errorMessage?: string
        }
      }> = []

      try {
        await waitFor(async () => isDaemonHealthy(port))
        const client = createDaemonIpcClient({
          daemonUrl: createDaemonUrl(port),
        })
        const abortController = new AbortController()
        const eventStream = await client.events.stream(
          {
            names: ["backend.stream.degraded"],
          },
          {
            signal: abortController.signal,
          },
        )
        const eventsDone = (async () => {
          for await (const event of eventStream) {
            degradedEvents.push(event as (typeof degradedEvents)[number])
          }
        })()
        unsubscribeEvents = async () => {
          abortController.abort()
          await eventsDone.catch(() => {})
        }

        streamResponse.resolve()
        await waitFor(() =>
          degradedEvents.some(
            (event) =>
              event.name === "backend.stream.degraded" &&
              event.payload?.reason === "stream_failed" &&
              typeof event.payload?.errorMessage === "string",
          ),
        )
        await stopDaemon()
        return await daemonPromise
      } finally {
        await unsubscribeEvents?.()
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(0)
  },
  { timeout: 10000 },
)

test(
  "daemon run keeps IPC available when stream startup is unauthenticated",
  async () => {
    await useTempHome()
    const streamResponse = createDeferred<void>()
    const backend = await startBackendHarness({
      beforeStreamResponse: () => streamResponse.promise,
      rejectStreamUnauthorized: true,
    })
    cleanup.push(() => backend.close())
    db.metadata.set("authToken", "tok")

    const port = await getUnusedTcpPort()

    const { result: exitCode } = await captureStdout(async () => {
      const daemonPromise = runDaemon({
        baseUrl: backend.baseUrl,
        port,
        agentBinDir,
        logMode: "json",
        store: db,
      })
      const stopDaemon = createDaemonStopper()
      let unsubscribeEvents: (() => Promise<void>) | undefined
      const degradedEvents: Array<{
        name?: string
        payload?: {
          reason?: string
          errorMessage?: string
        }
      }> = []

      try {
        await waitFor(async () => isDaemonHealthy(port))
        const client = createDaemonIpcClient({
          daemonUrl: createDaemonUrl(port),
        })
        const abortController = new AbortController()
        const eventStream = await client.events.stream(
          {
            names: ["backend.stream.degraded"],
          },
          {
            signal: abortController.signal,
          },
        )
        const eventsDone = (async () => {
          for await (const event of eventStream) {
            degradedEvents.push(event as (typeof degradedEvents)[number])
          }
        })()
        unsubscribeEvents = async () => {
          abortController.abort()
          await eventsDone.catch(() => {})
        }
        streamResponse.resolve()
        await waitFor(() =>
          degradedEvents.some(
            (event) =>
              event.name === "backend.stream.degraded" &&
              event.payload?.reason === "unauthenticated" &&
              typeof event.payload?.errorMessage === "string",
          ),
        )
        await stopDaemon()
        return await daemonPromise
      } finally {
        await unsubscribeEvents?.()
        await stopDaemon()
        await daemonPromise.catch(() => {})
      }
    })

    expect(exitCode).toBe(0)
    expect(backend.subscriptionCount()).toBe(0)
  },
  { timeout: 10000 },
)

test("daemon run defaults to compact terminal logs", async () => {
  const { output, result: exitCode } = await captureStdout(() =>
    runDaemon({
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
      store: db,
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
      store: db,
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
      store: db,
    }),
  )

  expect(exitCode).toBe(0)
  expect(output.some((line) => line.includes("daemon.startup"))).toBe(true)
  expect(output.some((line) => line.includes("baseUrl:"))).toBe(true)
  expect(output.every((line) => line.trim().startsWith("{"))).toBe(false)
})

test("daemon run logs startup failures after logging is configured", async () => {
  process.env.GODDARD_DAEMON_PORT = "not-a-port"

  const { logs, result: exitCode } = await captureJsonLogs(() =>
    runDaemon({
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
      logMode: "json",
      store: db,
    }),
  )

  expect(exitCode).toBe(1)
  expect(logs).toContainEqual(
    expect.objectContaining({
      event: "daemon.run_failed",
      errorName: "Error",
      errorMessage: "GODDARD_DAEMON_PORT must be an integer TCP port between 1 and 65535",
    }),
  )
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

test("daemon runtime defaults to the local backend URL", () => {
  delete process.env.GODDARD_BASE_URL

  expect(resolveRuntimeConfig({ port: 0 }).baseUrl).toBe("http://127.0.0.1:8787")
})

test("daemon runtime backend URL overrides preserve precedence", () => {
  process.env.GODDARD_BASE_URL = "http://127.0.0.1:9999"

  expect(resolveRuntimeConfig({ port: 0 }).baseUrl).toBe("http://127.0.0.1:9999")
  expect(resolveRuntimeConfig({ baseUrl: "https://example.test/api", port: 0 }).baseUrl).toBe(
    "https://example.test/api",
  )
})

async function runGit(cwd: string, args: string[]): Promise<void> {
  const subprocess = Bun.spawn(["git", ...args], {
    cwd,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  })
  const exitCode = await subprocess.exited
  expect(exitCode).toBe(0)
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
    await removeTemporaryPath(homeDir)
  })
  process.env.HOME = homeDir
  db = resetComposedDaemonStore({ filename: ":memory:" })
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
    await removeTemporaryPath(repoDir)
  })

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "repo", private: true }, null, 2),
    "utf-8",
  )

  await runGit(repoDir, ["init"])
  await runGit(repoDir, ["config", "user.email", "bot@example.com"])
  await runGit(repoDir, ["config", "user.name", "Bot"])
  await runGit(repoDir, ["add", "."])
  await runGit(repoDir, ["commit", "-m", "init"])

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
    beforeStreamResponse?: () => void | Promise<void>
    rejectStreamStatus?: number
    rejectStreamUnauthorized?: boolean
    isManaged?: (input: { owner: string; repo: string; prNumber: number }) => boolean
  } = {},
) {
  const streams = new Set<ServerResponse>()
  let subscriptionCount = 0
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "127.0.0.1"}`)

    if (url.pathname === "/events/stream") {
      request.resume()
      request.on("end", () => {
        void Promise.resolve()
          .then(() => options.beforeStreamResponse?.())
          .then(() => {
            if (options.rejectStreamUnauthorized || !request.headers.authorization) {
              response.writeHead(401, { "content-type": "text/plain" })
              response.end("unauthorized")
              return
            }

            if (options.rejectStreamStatus) {
              response.writeHead(options.rejectStreamStatus, { "content-type": "text/plain" })
              response.end("stream unavailable")
              return
            }

            subscriptionCount += 1
            response.writeHead(200, {
              "content-type": "application/x-ndjson; charset=utf-8",
              "cache-control": "no-cache",
              connection: "keep-alive",
            })
            response.flushHeaders()
            streams.add(response)
            response.on("close", () => {
              streams.delete(response)
            })
          })
          .catch((error) => {
            response.writeHead(500, { "content-type": "text/plain" })
            response.end(error instanceof Error ? error.message : String(error))
          })
      })
      return
    }

    if (url.pathname === "/pull-requests/managed") {
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
      const frame = `${JSON.stringify(event)}\n`
      for (const stream of streams) {
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
    const response = await client.daemon.health()
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

function createDeferred<T>() {
  let resolve: (value: T | PromiseLike<T>) => void = () => {}
  let reject: (reason?: unknown) => void = () => {}
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
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
