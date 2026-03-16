import { assert, test } from "vitest"
import { healthRoute, prReplyRoute, prSubmitRoute } from "../src/daemon/routes.ts"

test("daemon schema exports rouzer route declarations with stable paths", () => {
  assert.equal(healthRoute.path.source, "health")
  assert.equal(prSubmitRoute.path.source, "pr/submit")
  assert.equal(prReplyRoute.path.source, "pr/reply")
})
