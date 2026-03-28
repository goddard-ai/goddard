import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "vitest"
import type { DaemonServer } from "../src/ipc.ts"
import { startDaemonServer } from "../src/ipc.ts"
import { configureDaemonLogging } from "../src/logging.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("daemon submit request requires a valid session token", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const { logs } = await captureDaemonLogs(async () => {
    await expect(
      client.send("prSubmit", {
        token: "",
        cwd: process.cwd(),
        title: "Ship daemon security",
        body: "Done.",
      }),
    ).rejects.toThrow(/invalid session token/i)
  })

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const failed = logs.find((entry) => entry.event === "ipc.request_failed")
  expect(received?.requestName).toBe("prSubmit")
  expect(received?.payload).toEqual({
    token: "[REDACTED]",
    cwd: process.cwd(),
    title: "Ship daemon security",
    body: "Done.",
  })
  expect(received?.opId).toBe(failed?.opId)
  expect(failed?.requestName).toBe("prSubmit")
})

test("daemon submit request enforces trusted repo context and records created PR access", async () => {
  const createCalls: Array<Record<string, unknown>> = []
  const recordedPrs: Array<{ sessionId: string; prNumber: number }> = []
  const recordedLocations: Array<{
    owner: string
    repo: string
    prNumber: number
    cwd: string
  }> = []

  const daemon = await startTestDaemon({
    sdk: {
      auth: {
        startDeviceFlow: async () => ({
          deviceCode: "dev_1",
          userCode: "ABCD-1234",
          verificationUri: "https://github.com/login/device",
          expiresIn: 900,
          interval: 5,
        }),
        completeDeviceFlow: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
        logout: async () => {},
      },
      pr: {
        create: async (input) => {
          createCalls.push(input)
          return {
            number: 42,
            url: "https://github.com/trusted/widgets/pull/42",
          }
        },
        reply: async () => ({ success: true }),
      },
    },
    auth: {
      getSessionByToken: async (token) => {
        expect(token).toBe("tok_session")
        return {
          sessionId: "session-42",
          owner: "trusted",
          repo: "widgets",
          allowedPrNumbers: [],
        }
      },
      addAllowedPr: async (sessionId, prNumber) => {
        recordedPrs.push({ sessionId, prNumber })
      },
    },
    recordManagedPrLocation: async (record) => {
      recordedLocations.push(record)
      return {
        ...record,
        updatedAt: new Date().toISOString(),
      }
    },
    resolveSubmitRequest: async () => ({
      owner: "evil",
      repo: "fork",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureDaemonLogs(async () => {
    await client.send("prSubmit", {
      token: "tok_session",
      cwd: process.cwd(),
      title: "Ship daemon security",
      body: "Done.",
    })
  })

  expect(createCalls).toEqual([
    {
      owner: "trusted",
      repo: "widgets",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    },
  ])
  expect(recordedPrs).toEqual([{ sessionId: "session-42", prNumber: 42 }])
  expect(recordedLocations).toEqual([
    {
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
      cwd: process.cwd(),
    },
  ])

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const responded = logs.find((entry) => entry.event === "ipc.response_sent")
  expect(received?.requestName).toBe("prSubmit")
  expect(responded?.requestName).toBe("prSubmit")
  expect(received?.opId).toBe(responded?.opId)
  expect(responded?.sessionId).toBe("session-42")
})

test("daemon reply request rejects PRs outside the session allowlist", async () => {
  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async () => ({
        sessionId: "session-7",
        owner: "trusted",
        repo: "widgets",
        allowedPrNumbers: [7],
      }),
      addAllowedPr: async () => undefined,
    },
    resolveReplyRequest: async () => ({
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
      body: "Updated per review",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    client.send("prReply", {
      token: "tok_session",
      cwd: process.cwd(),
      message: "Updated per review",
    }),
  ).rejects.toThrow(/not allowed/i)
})

test("daemon reply request records managed PR checkout locations", async () => {
  const recordedLocations: Array<{
    owner: string
    repo: string
    prNumber: number
    cwd: string
  }> = []

  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async () => ({
        sessionId: "session-12",
        owner: "trusted",
        repo: "widgets",
        allowedPrNumbers: [12],
      }),
      addAllowedPr: async () => undefined,
    },
    recordManagedPrLocation: async (record) => {
      recordedLocations.push(record)
      return {
        ...record,
        updatedAt: new Date().toISOString(),
      }
    },
    resolveReplyRequest: async () => ({
      owner: "evil",
      repo: "fork",
      prNumber: 12,
      body: "Updated per review",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await client.send("prReply", {
    token: "tok_session",
    cwd: process.cwd(),
    message: "Updated per review",
  })

  expect(recordedLocations).toEqual([
    {
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
      cwd: process.cwd(),
    },
  ])
})

type StartTestDaemonOptions = {
  sdk?: {
    pr?: {
      create?: (input: any) => Promise<{ number: number; url: string }>
      reply?: (input: any) => Promise<{ success: boolean }>
    }
  }
  auth?: {
    getSessionByToken?: (token: string) => Promise<{
      sessionId: string
      owner: string
      repo: string
      allowedPrNumbers: number[]
    } | null>
    addAllowedPr?: (sessionId: string, prNumber: number) => Promise<void>
  }
  recordManagedPrLocation?: (record: {
    owner: string
    repo: string
    prNumber: number
    cwd: string
  }) => Promise<{
    owner: string
    repo: string
    prNumber: number
    cwd: string
    updatedAt: string
  }>
  resolveSubmitRequest?: (input: any) => Promise<any>
  resolveReplyRequest?: (input: any) => Promise<any>
}

async function startTestDaemon(options: StartTestDaemonOptions = {}): Promise<DaemonServer> {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-"))
  const socketPath = join(socketDir, "daemon.sock")

  const daemon = await startDaemonServer(
    {
      auth: {
        startDeviceFlow: async () => ({
          deviceCode: "dev_1",
          userCode: "ABCD-1234",
          verificationUri: "https://github.com/login/device",
          expiresIn: 900,
          interval: 5,
        }),
        completeDeviceFlow: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
        logout: async () => {},
      },
      pr: {
        create:
          options.sdk?.pr?.create ??
          (async () => ({
            number: 12,
            url: "https://github.com/trusted/widgets/pull/12",
          })),
        reply: options.sdk?.pr?.reply ?? (async () => ({ success: true })),
      },
    },
    { socketPath },
    {
      resolveSubmitRequest:
        options.resolveSubmitRequest ??
        (async () => ({
          owner: "trusted",
          repo: "widgets",
          title: "default",
          body: "",
          head: "feature/default",
          base: "main",
        })),
      resolveReplyRequest:
        options.resolveReplyRequest ??
        (async () => ({
          owner: "trusted",
          repo: "widgets",
          prNumber: 12,
          body: "reply",
        })),
      getSessionByToken: options.auth?.getSessionByToken,
      addAllowedPrToSession: options.auth?.addAllowedPr,
      recordManagedPrLocation: options.recordManagedPrLocation,
    },
  )

  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  return daemon
}

async function captureDaemonLogs(
  action: () => Promise<void>,
): Promise<{ logs: Array<Record<string, unknown>> }> {
  const output: string[] = []
  const restoreLogging = configureDaemonLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
  })

  try {
    await action()
    return {
      logs: output
        .flatMap((chunk) => chunk.split("\n"))
        .filter((line) => line.trim().length > 0)
        .map((line) => JSON.parse(line) as Record<string, unknown>),
    }
  } finally {
    restoreLogging()
  }
}
