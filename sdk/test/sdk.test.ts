import { test, expect } from "vitest"
import { promises as fs } from "node:fs"
import * as path from "node:path"
import * as os from "node:os"
import { createSdk, InMemoryTokenStorage } from "../src/index.ts"
import { init as agentsInit } from "../src/node/index.ts"

test("device flow stores token and whoami uses auth header", async () => {
  const storage = new InMemoryTokenStorage()

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input)
    if (url.endsWith("/auth/device/start")) {
      return jsonResponse(200, {
        deviceCode: "dev_1",
        userCode: "ABCD-1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      })
    }

    if (url.endsWith("/auth/device/complete")) {
      return jsonResponse(200, {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      })
    }

    if (url.endsWith("/auth/session")) {
      expect(init?.headers && (init.headers as Record<string, string>).authorization).toBe(
        "Bearer tok_1",
      )
      return jsonResponse(200, {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      })
    }

    return jsonResponse(404, { error: "not found" })
  }

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetchImpl,
  })

  const start = await sdk.auth.startDeviceFlow()
  expect(start.deviceCode).toBe("dev_1")

  const session = await sdk.auth.completeDeviceFlow({
    deviceCode: start.deviceCode,
    githubUsername: "alec",
  })
  expect(session.githubUsername).toBe("alec")
  expect(await storage.getToken()).toBe("tok_1")

  const me = await sdk.auth.whoami()
  expect(me.githubUserId).toBe(42)
})

test("pr create requires authentication", async () => {
  const storage = new InMemoryTokenStorage()

  const fetchImpl: typeof fetch = async (input) => {
    const url = String(input)
    if (url.endsWith("/pr/create")) {
      return jsonResponse(200, {
        id: 1,
        number: 1,
        owner: "org",
        repo: "repo",
        title: "demo",
        body: "body",
        head: "feat",
        base: "main",
        url: "https://github.com/org/repo/pull/1",
        createdBy: "alec",
        createdAt: new Date().toISOString(),
      })
    }

    return jsonResponse(404, { error: "not found" })
  }

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetchImpl,
  })

  await expect(() =>
    sdk.pr.create({ owner: "org", repo: "repo", title: "demo", head: "feat", base: "main" }),
  ).rejects.toThrow()

  await storage.setToken("tok_2")

  const pr = await sdk.pr.create({
    owner: "org",
    repo: "repo",
    title: "demo",
    head: "feat",
    base: "main",
  })
  expect(pr.number).toBe(1)
})

test("pr.isManaged returns managed status", async () => {
  const storage = new InMemoryTokenStorage()
  await storage.setToken("tok_pr")

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input)
    if (url.includes("/pr/managed?")) {
      expect(init?.headers && (init.headers as Record<string, string>).authorization).toBe(
        "Bearer tok_pr",
      )
      return jsonResponse(200, { managed: true })
    }
    return jsonResponse(404, { error: "not found" })
  }

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetchImpl,
  })

  const managed = await sdk.pr.isManaged({ owner: "org", repo: "repo", prNumber: 12 })
  expect(managed).toBe(true)
})

test("stream emits error event for malformed payloads", async () => {
  const storage = new InMemoryTokenStorage()
  await storage.setToken("tok_stream")

  const encoder = new TextEncoder()
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined

  const fetchImpl: typeof fetch = async (input) => {
    const url = String(input)
    if (url.includes("/stream?")) {
      const stream = new ReadableStream<Uint8Array>({
        start(ctrl) {
          controller = ctrl
        },
      })

      return new Response(stream, {
        status: 200,
        headers: {
          "content-type": "text/event-stream",
        },
      })
    }

    return jsonResponse(404, { error: "not found" })
  }

  const sdk = createSdk({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetchImpl,
  })

  const sub = await sdk.stream.subscribeToRepo({ owner: "org", repo: "repo" })

  let errorMessage = ""
  sub.on("error", (error) => {
    errorMessage = error instanceof Error ? error.message : String(error)
  })

  controller?.enqueue(encoder.encode("data: {\n\n"))
  await new Promise((resolve) => setTimeout(resolve, 0))

  expect(errorMessage).toMatch(/Invalid stream payload/)
  sub.close()
})

function jsonResponse(status: number, payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      "content-type": "application/json",
    },
  })
}

test("agents.init creates AGENTS.md with correct instructions", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-test-"))

  try {
    const { path: agentsPath } = await agentsInit(tempDir)
    expect(agentsPath).toBe(path.join(tempDir, "AGENTS.md"))

    const content = await fs.readFile(agentsPath, "utf-8")
    expect(content).toMatch(/The `spec` Folder/)
    expect(content).toMatch(/domain routing hub/)
  } finally {
    await fs.rm(tempDir, { recursive: true, force: true })
  }
})
