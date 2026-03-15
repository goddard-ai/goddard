import { beforeEach, expect, test, vi } from "vitest"

const { runAgentMock, spawnSyncMock } = vi.hoisted(() => ({
  runAgentMock: vi.fn(async () => null),
  spawnSyncMock: vi.fn(() => ({ status: 0 })),
}))

vi.mock("@goddard-ai/session", () => ({
  runAgent: runAgentMock,
}))

vi.mock("node:child_process", () => ({
  spawnSync: spawnSyncMock,
}))

import { runOneShot } from "../src/one-shot.ts"

beforeEach(() => {
  runAgentMock.mockClear()
  spawnSyncMock.mockClear()
})

test("runOneShot prepends daemon agent-bin to PATH before calling session runAgent", async () => {
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
    projectDir: "/tmp/project",
    env: {
      PATH: "/usr/bin:/bin",
      GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
      GODDARD_SESSION_TOKEN: "tok_session",
    },
  })

  expect(exitCode).toBe(0)
  expect(runAgentMock).toHaveBeenCalledTimes(1)

  const params = runAgentMock.mock.calls[0]?.[0]
  expect(params.env.GODDARD_DAEMON_URL).toBe("http://unix/?socketPath=%2Ftmp%2Fdaemon.sock")
  expect(params.env.GODDARD_SESSION_TOKEN).toBe("tok_session")
  expect(params.env.PATH).toContain("/usr/bin:/bin")
  expect(params.env.PATH).toContain("/daemon/agent-bin")
  expect(params.env.GODDARD_AGENT_BIN_DIR).toBeUndefined()
  expect(params.prompts).toBeUndefined()
  expect(params.systemPrompt).toContain("goddard")
  expect(params.systemPrompt).toContain("submit-pr")
})
