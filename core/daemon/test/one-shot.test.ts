import { beforeEach, expect, test, vi } from "vitest"

const { createDaemonIpcClientMock, sendMock } = vi.hoisted(() => ({
  sendMock: vi.fn(async () => ({ session: { id: "session-1" } })),
  createDaemonIpcClientMock: vi.fn(() => ({
    send: sendMock,
  })),
}))

vi.mock("@goddard-ai/daemon-client", async (importOriginal): Promise<typeof import("@goddard-ai/daemon-client")> => ({
  ...(await importOriginal<typeof import("@goddard-ai/daemon-client")>()),
  createDaemonIpcClient: createDaemonIpcClientMock,
  readSocketPathFromDaemonUrl: vi.fn((value: string) => value) as any,
}))

import { runOneShot } from "../src/one-shot.ts"

beforeEach(() => {
  sendMock.mockClear()
  createDaemonIpcClientMock.mockClear()
})

test("runOneShot creates a daemon-hosted one-shot session over IPC", async () => {
  const exitCode = await runOneShot({
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
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
    agentBinDir: "/tmp/goddard-agent-bin",
    resolveProjectDir: async () => "/tmp/project",
    env: {
      PATH: "/usr/bin:/bin",
    },
  })

  expect(exitCode).toBe(0)
  expect(createDaemonIpcClientMock).toHaveBeenCalledTimes(1)
  expect(sendMock).toHaveBeenCalledTimes(1)

  const [name, params] = sendMock.mock.calls[0] ?? []
  expect(name).toBe("sessionCreate")
  expect(params.agent).toBe("pi")
  expect(params.cwd).toBe("/tmp/project")
  expect(params.worktree).toEqual({ enabled: true })
  expect(params.oneShot).toBe(true)
  expect(params.initialPrompt).toBe("reply to feedback")
  expect(params.repository).toBe("acme/widgets")
  expect(params.prNumber).toBe(12)
  expect(params.metadata).toBeUndefined()
  expect(params.env.PATH).toContain("/usr/bin:/bin")
  expect(params.env.PATH).toContain("/tmp/goddard-agent-bin")
  expect(params.systemPrompt).toContain("goddard")
  expect(params.systemPrompt).toContain("submit-pr")
})
