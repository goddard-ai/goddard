import { afterEach, beforeEach, expect, test, vi } from "vitest"
import type { AgentLoopParams } from "@goddard-ai/schema/loop"

const mockedSession = vi.hoisted(() => {
  const promptMessages: string[] = []
  const promptPlans: Array<() => Promise<void> | void> = []
  const session = {
    prompt: vi.fn(async (message: string) => {
      promptMessages.push(message)
      await promptPlans.shift()?.()
    }),
    stop: vi.fn(),
    getHistory: vi.fn(async () => {
      throw new Error("getHistory should not be called")
    }),
  }
  const runAgentMock = vi.fn(async () => session)

  return {
    promptMessages,
    promptPlans,
    session,
    runAgentMock,
  }
})

vi.mock("@goddard-ai/session", () => ({
  runAgent: mockedSession.runAgentMock,
}))

import { runAgentLoop } from "../src/index.ts"

function createParams(
  overrides?: Partial<AgentLoopParams>,
): AgentLoopParams {
  return {
    nextPrompt: () => "default prompt",
    session: {
      agent: "pi-acp",
      cwd: process.cwd(),
      mcpServers: [],
    },
    rateLimits: {
      cycleDelay: "0s",
      maxOpsPerMinute: 100,
      maxCyclesBeforePause: 100,
      ...overrides?.rateLimits,
    },
    retries: {
      maxAttempts: 1,
      initialDelayMs: 1,
      maxDelayMs: 1,
      backoffFactor: 1,
      jitterRatio: 0,
      retryableErrors: () => false,
      ...overrides?.retries,
    },
    ...overrides,
  }
}

beforeEach(() => {
  mockedSession.promptMessages.length = 0
  mockedSession.promptPlans.length = 0
  mockedSession.session.prompt.mockClear()
  mockedSession.session.stop.mockClear()
  mockedSession.session.getHistory.mockClear()
  mockedSession.runAgentMock.mockClear()
})

afterEach(() => {
  vi.useRealTimers()
})

function createAbortError(): Error {
  const error = new Error("aborted")
  error.name = "AbortError"
  return error
}

test("prompts with the cycle number only", async () => {
  const nextPrompt = vi.fn(() => "custom prompt")
  mockedSession.promptPlans.push(() => {
    throw createAbortError()
  })

  await runAgentLoop(createParams({ nextPrompt }))

  await vi.waitFor(() => {
    expect(mockedSession.promptMessages).toHaveLength(1)
  })

  expect(nextPrompt).toHaveBeenCalledOnce()
  expect(nextPrompt).toHaveBeenCalledWith()
  expect(mockedSession.promptMessages[0]).toBe("custom prompt")
})

test("applies cycleDelay and maxCyclesBeforePause between cycles", async () => {
  vi.useFakeTimers()

  mockedSession.promptPlans.push(() => {})
  mockedSession.promptPlans.push(() => {})
  mockedSession.promptPlans.push(() => {
    throw createAbortError()
  })

  await runAgentLoop(
    createParams({
      rateLimits: {
        cycleDelay: "1s",
        maxOpsPerMinute: 100,
        maxCyclesBeforePause: 2,
      },
    }),
  )

  await vi.waitFor(() => {
    expect(mockedSession.promptMessages).toHaveLength(1)
  })

  await vi.advanceTimersByTimeAsync(999)
  expect(mockedSession.promptMessages).toHaveLength(1)

  await vi.advanceTimersByTimeAsync(1)
  await vi.waitFor(() => {
    expect(mockedSession.promptMessages).toHaveLength(2)
  })

  await vi.advanceTimersByTimeAsync(24 * 60 * 60 * 1000)
  expect(mockedSession.promptMessages).toHaveLength(2)

  await vi.advanceTimersByTimeAsync(1000)
  await vi.waitFor(() => {
    expect(mockedSession.promptMessages).toHaveLength(3)
  })
})

test("applies maxOpsPerMinute when cycles happen too quickly", async () => {
  vi.useFakeTimers()

  mockedSession.promptPlans.push(() => {})
  mockedSession.promptPlans.push(() => {})
  mockedSession.promptPlans.push(() => {
    throw createAbortError()
  })

  await runAgentLoop(
    createParams({
      rateLimits: {
        cycleDelay: "0s",
        maxOpsPerMinute: 1,
        maxCyclesBeforePause: 100,
      },
    }),
  )

  await vi.waitFor(() => {
    expect(mockedSession.promptMessages).toHaveLength(2)
  })

  await vi.advanceTimersByTimeAsync(1_000)
  expect(mockedSession.promptMessages).toHaveLength(2)

  await vi.advanceTimersByTimeAsync(60_000)
  expect(mockedSession.promptMessages.length).toBeGreaterThanOrEqual(3)
})
