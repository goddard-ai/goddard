import { expect, test } from "vitest"
import { daemonIpcSchema } from "../src/daemon-ipc.ts"

test("daemon IPC parses loop and workforce lifecycle payloads", () => {
  expect(
    daemonIpcSchema.client.requests.sessionCreate.payload.parse({
      agent: {
        id: "node-agent",
        name: "Node Agent",
        version: "1.0.0",
        description: "Local node-based ACP test agent.",
        distribution: {
          npx: {
            package: "@example/node-agent",
            args: ["agent.mjs"],
          },
        },
      },
      cwd: "/repo",
      mcpServers: [],
      systemPrompt: "Follow the spec.",
    }),
  ).toEqual({
    agent: {
      id: "node-agent",
      name: "Node Agent",
      version: "1.0.0",
      description: "Local node-based ACP test agent.",
      distribution: {
        npx: {
          package: "@example/node-agent",
          args: ["agent.mjs"],
        },
      },
    },
    cwd: "/repo",
    mcpServers: [],
    systemPrompt: "Follow the spec.",
  })

  expect(
    daemonIpcSchema.client.requests.loopStart.payload.parse({
      rootDir: "/repo",
      loopName: "review",
      promptModulePath: "/repo/.goddard/loops/review/prompt.js",
      session: {
        agent: "pi-acp",
        cwd: "/repo",
        mcpServers: [],
        systemPrompt: "Follow the spec.",
      },
      rateLimits: {
        cycleDelay: "30s",
        maxOpsPerMinute: 4,
        maxCyclesBeforePause: 200,
      },
      retries: {
        maxAttempts: 1,
        initialDelayMs: 500,
        maxDelayMs: 5_000,
        backoffFactor: 2,
        jitterRatio: 0.2,
      },
    }),
  ).toEqual({
    rootDir: "/repo",
    loopName: "review",
    promptModulePath: "/repo/.goddard/loops/review/prompt.js",
    session: {
      agent: "pi-acp",
      cwd: "/repo",
      mcpServers: [],
      systemPrompt: "Follow the spec.",
    },
    rateLimits: {
      cycleDelay: "30s",
      maxOpsPerMinute: 4,
      maxCyclesBeforePause: 200,
    },
    retries: {
      maxAttempts: 1,
      initialDelayMs: 500,
      maxDelayMs: 5_000,
      backoffFactor: 2,
      jitterRatio: 0.2,
    },
  })

  expect(
    daemonIpcSchema.client.requests.workforceRequest.payload.parse({
      rootDir: "/repo",
      targetAgentId: "api",
      input: "Ship it.",
      intent: "create",
      token: "tok_1",
    }),
  ).toEqual({
    rootDir: "/repo",
    targetAgentId: "api",
    input: "Ship it.",
    intent: "create",
    token: "tok_1",
  })
})
