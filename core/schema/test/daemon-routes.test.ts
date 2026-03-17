import { assert, test } from "vitest"
import {
  healthRoute,
  prReplyRoute,
  prSubmitRoute,
  sessionCreateRoute,
  sessionGetRoute,
  sessionHistoryRoute,
  sessionShutdownRoute,
} from "../src/daemon/routes.ts"

test("daemon routes keep their stable public paths", () => {
  assert.equal(healthRoute.path.source, "health")
  assert.equal(prSubmitRoute.path.source, "pr/submit")
  assert.equal(prReplyRoute.path.source, "pr/reply")
  assert.equal(sessionCreateRoute.path.source, "sessions")
  assert.equal(sessionGetRoute.path.source, "sessions/:id")
  assert.equal(sessionHistoryRoute.path.source, "sessions/:id/history")
  assert.equal(sessionShutdownRoute.path.source, "sessions/:id/shutdown")
})

test("daemon PR routes parse representative auth and body contracts", () => {
  assert.deepEqual(prSubmitRoute.methods.POST?.headers?.parse({ authorization: "Bearer tok_1" }), {
    authorization: "Bearer tok_1",
  })
  assert.deepEqual(
    prSubmitRoute.methods.POST?.body?.parse({
      cwd: "/tmp/project",
      title: "Ship daemon routing",
      body: "Done.",
      head: "feat/daemon-routing",
      base: "main",
    }),
    {
      cwd: "/tmp/project",
      title: "Ship daemon routing",
      body: "Done.",
      head: "feat/daemon-routing",
      base: "main",
    },
  )
  assert.deepEqual(
    prReplyRoute.methods.POST?.body?.parse({
      cwd: "/tmp/project",
      message: "Updated per review.",
    }),
    {
      cwd: "/tmp/project",
      message: "Updated per review.",
    },
  )
})

test("daemon session routes parse representative creation and path contracts", () => {
  assert.deepEqual(
    sessionCreateRoute.methods.POST?.body?.parse({
      agent: {
        type: "binary",
        cmd: "node",
        args: ["agent.mjs"],
      },
      cwd: "/tmp/project",
      mcpServers: [],
      systemPrompt: "Follow the repository conventions.",
      env: { PATH: "/usr/bin" },
      metadata: { repository: "goddard-ai/sdk", prNumber: 12 },
      initialPrompt: "Ship it.",
      oneShot: true,
    }),
    {
      agent: {
        type: "binary",
        cmd: "node",
        args: ["agent.mjs"],
      },
      cwd: "/tmp/project",
      mcpServers: [],
      systemPrompt: "Follow the repository conventions.",
      env: { PATH: "/usr/bin" },
      metadata: { repository: "goddard-ai/sdk", prNumber: 12 },
      initialPrompt: "Ship it.",
      oneShot: true,
    },
  )
  assert.deepEqual(sessionGetRoute.methods.GET?.path?.parse({ id: "session-1" }), {
    id: "session-1",
  })
  assert.deepEqual(sessionHistoryRoute.methods.GET?.path?.parse({ id: "session-1" }), {
    id: "session-1",
  })
  assert.deepEqual(sessionShutdownRoute.methods.POST?.path?.parse({ id: "session-1" }), {
    id: "session-1",
  })
})
