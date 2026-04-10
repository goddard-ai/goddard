import { expect, test } from "bun:test"
import { buildCreateSessionInput } from "./session-launch.ts"

test("buildCreateSessionInput trims the launch prompt and carries the fixed daemon session defaults", () => {
  expect(buildCreateSessionInput("/repo-a", "  Review the current diff.  ")).toEqual({
    agent: "pi",
    cwd: "/repo-a",
    mcpServers: [],
    systemPrompt: "",
    initialPrompt: "Review the current diff.",
  })
})
