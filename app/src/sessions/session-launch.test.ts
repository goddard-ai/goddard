import { expect, test } from "vitest"
import { SessionLaunch } from "./session-launch.ts"

test("createSessionInput trims the launch prompt and carries the fixed daemon session defaults", () => {
  const sessionLaunch = new SessionLaunch()

  sessionLaunch.openDialog("/repo-a")
  sessionLaunch.setDraftPrompt("  Review the current diff.  ")

  expect(sessionLaunch.createSessionInput()).toEqual({
    agent: "pi",
    cwd: "/repo-a",
    mcpServers: [],
    systemPrompt: "",
    initialPrompt: "Review the current diff.",
  })
})
