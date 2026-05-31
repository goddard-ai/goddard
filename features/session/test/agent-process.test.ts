import { expect, test } from "bun:test"

import { buildAgentProcessEnv } from "../src/daemon/agent-process.ts"

const createAgentEnvironment = ({ env }: { env?: Record<string, string> }) => ({
  ...env,
  PATH: ["/agent-bin", env?.PATH].filter(Boolean).join(":"),
})

test("agent process env preserves current inheritance and deterministic env precedence", () => {
  const env = buildAgentProcessEnv({
    daemonUrl: "http://127.0.0.1:8787",
    token: "tok_1",
    createAgentEnvironment,
    hostEnv: {
      HOST_ONLY: "host",
      SECRET: "host-secret",
      PATH: "/usr/bin",
    },
    agentEnv: {
      AGENT_ONLY: "agent",
      OVERRIDE: "agent",
    },
    sessionEnv: {
      OVERRIDE: "session",
    },
    envPolicy: {
      block: ["SECRET"],
    },
  })

  expect(env.HOST_ONLY).toBe("host")
  expect(env.AGENT_ONLY).toBe("agent")
  expect(env.OVERRIDE).toBe("session")
  expect(env.SECRET).toBeUndefined()
  expect(env.PATH).toBe("/agent-bin:/usr/bin")
  expect(env.GODDARD_DAEMON_URL).toBe("http://127.0.0.1:8787")
  expect(env.GODDARD_SESSION_TOKEN).toBe("tok_1")
})

test("agent process env policy can disable host inheritance and allow fixed injected values", () => {
  const env = buildAgentProcessEnv({
    daemonUrl: "http://127.0.0.1:8787",
    token: "tok_1",
    createAgentEnvironment,
    hostEnv: {
      HOST_ONLY: "host",
      PATH: "/usr/bin",
    },
    agentEnv: {
      AGENT_ONLY: "agent",
      PATH: "/custom/bin",
    },
    sessionEnv: {
      REQUEST_ONLY: "request",
      BLOCKED_REQUEST: "request-secret",
    },
    envPolicy: {
      inherit: false,
      allow: ["FIXED", "PATH", "REQUEST_ONLY"],
      block: ["BLOCKED_REQUEST"],
      set: {
        FIXED: "global",
        BLOCKED_REQUEST: "global-secret",
      },
    },
  })

  expect(env.HOST_ONLY).toBeUndefined()
  expect(env.AGENT_ONLY).toBeUndefined()
  expect(env.FIXED).toBe("global")
  expect(env.REQUEST_ONLY).toBe("request")
  expect(env.BLOCKED_REQUEST).toBeUndefined()
  expect(env.PATH).toBe("/agent-bin:/custom/bin")
  expect(env.GODDARD_DAEMON_URL).toBe("http://127.0.0.1:8787")
  expect(env.GODDARD_SESSION_TOKEN).toBe("tok_1")
})
