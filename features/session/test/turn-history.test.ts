import { expect, test } from "bun:test"

import { coalesceSessionHistoryMessages } from "../src/daemon/turn-history.ts"

function createTextUpdate(
  sessionUpdate: "agent_message_chunk" | "agent_thought_chunk",
  text: string,
) {
  return {
    jsonrpc: "2.0" as const,
    method: "session/update" as const,
    params: {
      sessionId: "acp-1",
      update: {
        sessionUpdate,
        content: {
          type: "text",
          text,
        },
      },
    },
  }
}

test("coalesceSessionHistoryMessages folds adjacent agent thought chunks", () => {
  expect(
    coalesceSessionHistoryMessages([
      createTextUpdate("agent_thought_chunk", "Read "),
      createTextUpdate("agent_thought_chunk", "the trace."),
    ]),
  ).toEqual([createTextUpdate("agent_thought_chunk", "Read the trace.")])
})

test("coalesceSessionHistoryMessages keeps thought and message chunks separate", () => {
  expect(
    coalesceSessionHistoryMessages([
      createTextUpdate("agent_thought_chunk", "Considered the patch."),
      createTextUpdate("agent_message_chunk", "Applied the patch."),
    ]),
  ).toEqual([
    createTextUpdate("agent_thought_chunk", "Considered the patch."),
    createTextUpdate("agent_message_chunk", "Applied the patch."),
  ])
})
