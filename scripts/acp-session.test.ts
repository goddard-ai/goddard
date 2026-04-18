import { describe, expect, test } from "bun:test"

import {
  collectSessionUpdates,
  getPromptCompletionMessage,
  isResumeExitCommand,
} from "./acp-session.ts"

describe("collectSessionUpdates", () => {
  test("flattens paged history turns and keeps only session/update notifications", () => {
    expect(
      collectSessionUpdates([
        {
          turnId: "turn-1",
          sequence: 0,
          promptRequestId: "prompt-1",
          startedAt: "2026-04-17T00:00:00.000Z",
          completedAt: "2026-04-17T00:00:01.000Z",
          completionKind: "result",
          stopReason: "end_turn",
          messages: [
            {
              jsonrpc: "2.0",
              id: "req-1",
              method: "session/prompt",
              params: { sessionId: "acp-1", prompt: [{ type: "text", text: "hello" }] },
            },
            {
              jsonrpc: "2.0",
              method: "session/update",
              params: { sessionId: "acp-1", update: { sessionUpdate: "session_info_update" } },
            },
          ],
        },
        {
          turnId: "turn-2",
          sequence: 1,
          promptRequestId: "prompt-2",
          startedAt: "2026-04-17T00:00:02.000Z",
          completedAt: null,
          completionKind: null,
          stopReason: null,
          messages: [
            {
              jsonrpc: "2.0",
              method: "session/update",
              params: { sessionId: "acp-1", update: { sessionUpdate: "agent_message_chunk" } },
            },
          ],
        },
      ]),
    ).toEqual([
      {
        jsonrpc: "2.0",
        method: "session/update",
        params: { sessionId: "acp-1", update: { sessionUpdate: "session_info_update" } },
      },
      {
        jsonrpc: "2.0",
        method: "session/update",
        params: { sessionId: "acp-1", update: { sessionUpdate: "agent_message_chunk" } },
      },
    ])
  })
})

describe("getPromptCompletionMessage", () => {
  test("returns the matching result payload for a prompt request id", () => {
    expect(
      getPromptCompletionMessage(
        {
          jsonrpc: "2.0",
          id: "prompt-1",
          result: {
            stopReason: "end_turn",
          },
        },
        "prompt-1",
      ),
    ).toEqual({
      kind: "result",
      message: {
        jsonrpc: "2.0",
        id: "prompt-1",
        result: {
          stopReason: "end_turn",
        },
      },
    })
  })

  test("returns null for non-terminal or mismatched messages", () => {
    expect(
      getPromptCompletionMessage(
        {
          jsonrpc: "2.0",
          method: "session/update",
          params: {
            sessionId: "acp-1",
            update: { sessionUpdate: "agent_message_chunk" },
          },
        },
        "prompt-1",
      ),
    ).toBeNull()

    expect(
      getPromptCompletionMessage(
        {
          jsonrpc: "2.0",
          id: "prompt-2",
          error: {
            code: -32000,
            message: "boom",
          },
        },
        "prompt-1",
      ),
    ).toBeNull()
  })
})

describe("isResumeExitCommand", () => {
  test("accepts the supported detach commands", () => {
    expect(isResumeExitCommand("/exit")).toBe(true)
    expect(isResumeExitCommand(" /quit ")).toBe(true)
    expect(isResumeExitCommand("continue")).toBe(false)
  })
})
