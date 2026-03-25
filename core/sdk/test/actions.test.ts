import { expect, test, vi } from "vitest"
import { runAgentAction } from "../src/daemon/actions.ts"

test("runAgentAction forwards named action execution to daemon IPC", async () => {
  const send = vi.fn(async () => ({
    session: {
      id: "session-1",
    },
  }))

  await expect(
    runAgentAction(
      "review",
      {
        cwd: "/repo",
        systemPrompt: "Use the review checklist.",
      },
      {
        client: { send } as never,
      },
    ),
  ).resolves.toBeNull()

  expect(send).toHaveBeenCalledWith("actionRun", {
    actionName: "review",
    cwd: "/repo",
    agent: undefined,
    mcpServers: undefined,
    env: undefined,
    systemPrompt: "Use the review checklist.",
    repository: undefined,
    prNumber: undefined,
    metadata: undefined,
  })
})
