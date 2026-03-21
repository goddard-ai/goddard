import { assert, test } from "vitest"
import {
  healthRoute,
  prReplyRoute,
  prSubmitRoute,
  sessionCreateRoute,
  sessionGetRoute,
  sessionHistoryRoute,
  sessionListRoute,
  sessionShutdownRoute,
} from "../src/daemon/routes.ts"

test("daemon routes keep their stable public paths", () => {
  assert.equal(healthRoute.path.source, "health")
  assert.equal(prSubmitRoute.path.source, "pr/submit")
  assert.equal(prReplyRoute.path.source, "pr/reply")
  assert.equal(sessionListRoute.path.source, "sessions")
  assert.equal(sessionCreateRoute.path.source, "sessions")
  assert.equal(sessionGetRoute.path.source, "sessions/:id")
  assert.equal(sessionHistoryRoute.path.source, "sessions/:id/history")
  assert.equal(sessionShutdownRoute.path.source, "sessions/:id/shutdown")
})
