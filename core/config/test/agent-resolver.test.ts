import { afterEach, expect, test } from "bun:test"

import { resolveDefaultAgent } from "../src/agent-resolver.ts"

const originalPath = process.env.PATH

afterEach(() => {
  if (originalPath === undefined) {
    delete process.env.PATH
  } else {
    process.env.PATH = originalPath
  }
})

test("resolveDefaultAgent uses agents.default after narrower session config", async () => {
  await expect(
    resolveDefaultAgent({
      agents: {
        default: "global-agent",
      },
    }),
  ).resolves.toBe("global-agent")

  await expect(
    resolveDefaultAgent({
      agents: {
        default: "global-agent",
      },
      session: {
        agent: "session-agent",
      },
    }),
  ).resolves.toBe("session-agent")
})

test("resolveDefaultAgent uses feature-specific session agents before agents.default", async () => {
  await expect(
    resolveDefaultAgent(
      {
        agents: {
          default: "global-agent",
        },
        session: {
          agent: "session-agent",
        },
        actions: {
          session: {
            agent: "action-agent",
          },
        },
      },
      "actions",
    ),
  ).resolves.toBe("action-agent")

  await expect(
    resolveDefaultAgent(
      {
        agents: {
          default: "global-agent",
        },
        session: {
          agent: "session-agent",
        },
        loops: {
          session: {
            agent: "loop-agent",
          },
        },
      },
      "loops",
    ),
  ).resolves.toBe("loop-agent")
})

test("resolveDefaultAgent rejects when no configured or discoverable agent exists", async () => {
  process.env.PATH = ""

  await expect(resolveDefaultAgent()).rejects.toThrow(
    "No default ACP agent is configured or discoverable.",
  )
})
