import { afterEach, test } from "vitest"
import * as assert from "node:assert/strict"
import { mkdtemp, rm } from "node:fs/promises"
import { request as httpRequest } from "node:http"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { DaemonServer } from "../src/ipc.ts"
import { startDaemonServer } from "../src/ipc.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("daemon submit endpoint requires a bearer session token", async () => {
  const daemon = await startTestDaemon()

  const response = await requestJson({
    socketPath: daemon.socketPath,
    path: "/pr/submit",
    method: "POST",
    body: {
      cwd: process.cwd(),
      title: "Ship daemon security",
      body: "Done.",
    },
  })

  assert.equal(response.statusCode, 401)
  assert.match(response.body.error, /authorization/i)
})

test("daemon submit endpoint enforces trusted repo context and records created PR access", async () => {
  const createCalls: Array<Record<string, unknown>> = []
  const recordedPrs: Array<{ sessionId: string; prNumber: number }> = []

  const daemon = await startTestDaemon({
    sdk: {
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
        assert.equal(token, "tok_session")
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
    resolveSubmitRequest: async () => ({
      owner: "evil",
      repo: "fork",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    }),
  })

  const response = await requestJson({
    socketPath: daemon.socketPath,
    path: "/pr/submit",
    method: "POST",
    token: "tok_session",
    body: {
      cwd: process.cwd(),
      title: "Ship daemon security",
      body: "Done.",
    },
  })

  assert.equal(response.statusCode, 200)
  assert.deepEqual(createCalls, [
    {
      owner: "trusted",
      repo: "widgets",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    },
  ])
  assert.deepEqual(recordedPrs, [{ sessionId: "session-42", prNumber: 42 }])
})

test("daemon reply endpoint rejects PRs outside the session allowlist", async () => {
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

  const response = await requestJson({
    socketPath: daemon.socketPath,
    path: "/pr/reply",
    method: "POST",
    token: "tok_session",
    body: {
      cwd: process.cwd(),
      message: "Updated per review",
    },
  })

  assert.equal(response.statusCode, 403)
  assert.match(response.body.error, /allowed/i)
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
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-"))
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

async function requestJson(input: {
  socketPath: string
  path: string
  method: "POST"
  body: Record<string, unknown>
  token?: string
}): Promise<{ statusCode: number; body: any }> {
  const payload = JSON.stringify(input.body)

  return new Promise((resolve, reject) => {
    const request = httpRequest(
      {
        socketPath: input.socketPath,
        path: input.path,
        method: input.method,
        headers: {
          "content-type": "application/json",
          "content-length": Buffer.byteLength(payload),
          ...(input.token ? { authorization: `Bearer ${input.token}` } : {}),
        },
      },
      (response) => {
        let raw = ""
        response.setEncoding("utf8")
        response.on("data", (chunk) => {
          raw += chunk
        })
        response.on("end", () => {
          resolve({
            statusCode: response.statusCode ?? 0,
            body: raw ? JSON.parse(raw) : {},
          })
        })
      },
    )

    request.once("error", reject)
    request.write(payload)
    request.end()
  })
}
