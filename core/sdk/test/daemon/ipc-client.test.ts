import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { startDaemonServer, type DaemonServer } from "../../../daemon/src/ipc.js"
import { vi } from "vitest"

vi.mock("@goddard-ai/config", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@goddard-ai/config")>()
  return {
    ...actual,
    resolveDefaultAgent: vi.fn().mockResolvedValue("pi-acp"),
  }
})
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, assert, test } from "vitest"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("SDK daemon IPC client can submit PR through the daemon request map", async () => {
  const createCalls: Array<Record<string, unknown>> = []
  const daemon = await startTestDaemon({
    sdk: {
      pr: {
        create: async (input) => {
          createCalls.push(input)
          return {
            number: 99,
            url: "https://github.com/trusted/widgets/pull/99",
          }
        },
      },
    },
    auth: {
      getSessionByToken: async (token) => {
        assert.equal(token, "tok_session")
        return {
          sessionId: "session-99",
          owner: "trusted",
          repo: "widgets",
          allowedPrNumbers: [],
        }
      },
      addAllowedPr: async () => undefined,
    },
    resolveSubmitRequest: async () => ({
      owner: "evil",
      repo: "fork",
      title: "Ship daemon route client",
      body: "Done",
      head: "feature/daemon-client",
      base: "main",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const result = await client.send("prSubmit", {
    token: "tok_session",
    cwd: process.cwd(),
    title: "Ship daemon route client",
    body: "Done",
  })

  assert.deepEqual(result, {
    number: 99,
    url: "https://github.com/trusted/widgets/pull/99",
  })
  assert.deepEqual(createCalls, [
    {
      owner: "trusted",
      repo: "widgets",
      title: "Ship daemon route client",
      body: "Done",
      head: "feature/daemon-client",
      base: "main",
    },
  ])
})

test("SDK daemon IPC client can reply to allowed PRs through the daemon request map", async () => {
  const replyCalls: Array<Record<string, unknown>> = []
  const daemon = await startTestDaemon({
    sdk: {
      pr: {
        reply: async (input) => {
          replyCalls.push(input)
          return { success: true }
        },
      },
    },
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
      prNumber: 7,
      body: "Updated per review",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const result = await client.send("prReply", {
    token: "tok_session",
    cwd: process.cwd(),
    message: "Updated per review",
  })

  assert.deepEqual(result, { success: true })
  assert.deepEqual(replyCalls, [
    {
      owner: "trusted",
      repo: "widgets",
      prNumber: 7,
      body: "Updated per review",
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
  resolveSubmitRequest?: (input: any) => Promise<any>
  resolveReplyRequest?: (input: any) => Promise<any>
}

async function startTestDaemon(options: StartTestDaemonOptions = {}): Promise<DaemonServer> {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-daemon-client-"))
  const socketPath = join(socketDir, "daemon.sock")

  const daemon = await startDaemonServer(
    {
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
    },
  )

  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  return daemon
}
