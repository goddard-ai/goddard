import type * as acp from "acp-client/protocol"
import { expect, test } from "bun:test"

import { coalesceSessionHistoryMessages } from "../src/daemon/turn-history.ts"
import type { SessionTurnMessage } from "../src/schema.ts"

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

function turnMessage(
  sequenceStart: number,
  sequence: number,
  message: acp.AnyMessage,
): SessionTurnMessage {
  return {
    sequenceStart,
    sequence,
    message,
  }
}

test("coalesceSessionHistoryMessages folds adjacent agent thought chunks", () => {
  expect(
    coalesceSessionHistoryMessages([
      createTextUpdate("agent_thought_chunk", "Read "),
      createTextUpdate("agent_thought_chunk", "the trace."),
    ]),
  ).toEqual([turnMessage(0, 1, createTextUpdate("agent_thought_chunk", "Read the trace."))])
})

test("coalesceSessionHistoryMessages keeps thought and message chunks separate", () => {
  expect(
    coalesceSessionHistoryMessages([
      createTextUpdate("agent_thought_chunk", "Considered the patch."),
      createTextUpdate("agent_message_chunk", "Applied the patch."),
    ]),
  ).toEqual([
    turnMessage(0, 0, createTextUpdate("agent_thought_chunk", "Considered the patch.")),
    turnMessage(1, 1, createTextUpdate("agent_message_chunk", "Applied the patch.")),
  ])
})

test("coalesceSessionHistoryMessages does not fold across a tool boundary", () => {
  const toolCall = {
    jsonrpc: "2.0" as const,
    method: "session/update" as const,
    params: {
      sessionId: "acp-1",
      update: {
        sessionUpdate: "tool_call" as const,
        toolCallId: "tool-1",
        title: "Read",
      },
    },
  }

  expect(
    coalesceSessionHistoryMessages([
      createTextUpdate("agent_thought_chunk", "Before "),
      toolCall,
      createTextUpdate("agent_thought_chunk", "after."),
    ]),
  ).toEqual([
    turnMessage(0, 0, createTextUpdate("agent_thought_chunk", "Before ")),
    turnMessage(1, 1, toolCall),
    turnMessage(2, 2, createTextUpdate("agent_thought_chunk", "after.")),
  ])
})
