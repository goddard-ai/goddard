import { beforeEach, expect, test, vi } from "vitest"

const { createDaemonIpcClientMock, sendMock, spawnSyncMock } = vi.hoisted(() => ({
  sendMock: vi.fn(async () => ({ session: { id: "session-1" } })),
  createDaemonIpcClientMock: vi.fn(() => ({
    send: sendMock,
  })),
  spawnSyncMock: vi.fn(() => ({ status: 0 })),
}))

vi.mock("@goddard-ai/daemon-client", () => ({
  createDaemonIpcClient: createDaemonIpcClientMock,
  readSocketPathFromDaemonUrl: vi.fn((value: string) => value),
}))

vi.mock("node:child_process", () => ({
  spawnSync: spawnSyncMock,
}))

import { runOneShot } from "../src/one-shot.ts"

beforeEach(() => {
  sendMock.mockClear()
  createDaemonIpcClientMock.mockClear()
  spawnSyncMock.mockClear()
})

test("runOneShot creates a daemon-hosted one-shot session over IPC", async () => {
  const sessionId = await runOneShot({
    event: {
      type: "comment",
      owner: "acme",
      repo: "widgets",
      prNumber: 12,
      author: "alice",
      body: "please update",
      reactionAdded: "eyes",
      createdAt: new Date().toISOString(),
    },
    prompt: "reply to feedback",
    projectDir: "/tmp/project",
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    agentBinDir: "/tmp/goddard-agent-bin",
    env: {
      PATH: "/usr/bin:/bin",
    },
  })

  expect(sessionId).toBe("session-1")
  expect(createDaemonIpcClientMock).toHaveBeenCalledTimes(1)
  expect(sendMock).toHaveBeenCalledTimes(1)

  const [name, params] = sendMock.mock.calls[0] ?? []
  expect(name).toBe("sessionCreate")
  expect(params.agent).toBe("pi")
  expect(params.oneShot).toBe(true)
  expect(params.initialPrompt).toBe("reply to feedback")
  expect(params.metadata).toEqual({ repository: "acme/widgets", prNumber: 12 })
  expect(params.env.PATH).toContain("/usr/bin:/bin")
  expect(params.env.PATH).toContain("/tmp/goddard-agent-bin")
  expect(params.systemPrompt).toContain("goddard")
  expect(params.systemPrompt).toContain("submit-pr")
})
