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

test("daemon schema exports rouzer route declarations with stable paths", () => {
  assert.equal(healthRoute.path.source, "health")
  assert.equal(prSubmitRoute.path.source, "pr/submit")
  assert.equal(prReplyRoute.path.source, "pr/reply")
  assert.equal(sessionCreateRoute.path.source, "sessions")
  assert.equal(sessionGetRoute.path.source, "sessions/:id")
  assert.equal(sessionHistoryRoute.path.source, "sessions/:id/history")
  assert.equal(sessionShutdownRoute.path.source, "sessions/:id/shutdown")
})

test("daemon session routes remain keyed by internal sessions.id, not ACP session ids", () => {
  assert.equal(sessionGetRoute.path.source.includes(":id"), true)
  assert.equal(sessionHistoryRoute.path.source.includes(":id"), true)
  assert.equal(sessionShutdownRoute.path.source.includes(":id"), true)
})
