import { expect, test } from "bun:test"

import { SessionTurnMessage, sessionTurnMessageCoversSequence } from "../src/schema.ts"

test("SessionTurnMessage represents one uncoalesced turn message sequence", () => {
  const message = SessionTurnMessage.parse({
    sequence: 7,
    sequenceStart: 7,
    message: {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "acp-1",
        update: {
          sessionUpdate: "tool_call",
          toolCallId: "tool-1",
        },
      },
    },
  })

  expect(sessionTurnMessageCoversSequence(message, 6)).toBe(false)
  expect(sessionTurnMessageCoversSequence(message, 7)).toBe(true)
  expect(sessionTurnMessageCoversSequence(message, 8)).toBe(false)
})

test("SessionTurnMessage represents a coalesced adjacent text sequence range", () => {
  const message = SessionTurnMessage.parse({
    sequence: 11,
    sequenceStart: 10,
    message: {
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "acp-1",
        update: {
          sessionUpdate: "agent_thought_chunk",
          content: {
            type: "text",
            text: "Read the trace.",
          },
        },
      },
    },
  })

  expect(sessionTurnMessageCoversSequence(message, 9)).toBe(false)
  expect(sessionTurnMessageCoversSequence(message, 10)).toBe(true)
  expect(sessionTurnMessageCoversSequence(message, 11)).toBe(true)
  expect(sessionTurnMessageCoversSequence(message, 12)).toBe(false)
})
