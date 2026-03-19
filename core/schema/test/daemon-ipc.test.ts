import * as assert from "node:assert/strict"
import { test } from "vitest"
import { daemonIpcSchema } from "../src/daemon-ipc.ts"

test("daemon IPC parses workforce lifecycle and mutation payloads", () => {
  assert.deepEqual(
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
    {
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
    },
  )

  assert.deepEqual(
    daemonIpcSchema.client.requests.workforceStart.payload.parse({
      rootDir: "/repo",
    }),
    {
      rootDir: "/repo",
    },
  )

  assert.deepEqual(
    daemonIpcSchema.client.requests.workforceRequest.payload.parse({
      rootDir: "/repo",
      targetAgentId: "api",
      input: "Ship it.",
      intent: "create",
      token: "tok_1",
    }),
    {
      rootDir: "/repo",
      targetAgentId: "api",
      input: "Ship it.",
      intent: "create",
      token: "tok_1",
    },
  )

  assert.deepEqual(
    daemonIpcSchema.client.requests.workforceSuspend.payload.parse({
      rootDir: "/repo",
      requestId: "req-1",
      reason: "Need a root decision.",
      token: "tok_1",
    }),
    {
      rootDir: "/repo",
      requestId: "req-1",
      reason: "Need a root decision.",
      token: "tok_1",
    },
  )
})
