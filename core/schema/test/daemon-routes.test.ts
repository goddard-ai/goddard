import { assert, test } from "vitest"
import {
  healthRoute,
  prReplyRoute,
  prSubmitRoute,
  sessionsAcpWsRoute,
  sessionsCreateRoute,
  sessionsGetRoute,
  sessionsHistoryRoute,
  sessionsShutdownRoute,
} from "../src/daemon/routes.ts"

test("daemon schema exports rouzer route declarations with stable paths", () => {
  assert.equal(healthRoute.path.source, "health")
  assert.equal(prSubmitRoute.path.source, "pr/submit")
  assert.equal(prReplyRoute.path.source, "pr/reply")
})

test("daemon schema exports rouzer route declarations with stable paths for sessions", () => {
  assert.equal(sessionsCreateRoute.path.source, "sessions")
  assert.equal(sessionsGetRoute.path.source, "sessions/:id")
  assert.equal(sessionsHistoryRoute.path.source, "sessions/:id/history")
  assert.equal(sessionsShutdownRoute.path.source, "sessions/:id/shutdown")
  assert.equal(sessionsAcpWsRoute.path.source, "sessions/:id/acp")
})

test.todo("daemon session APIs are keyed by internal id")
