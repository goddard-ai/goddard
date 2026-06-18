import { expect, test } from "bun:test"

import { resolveSessionOriginVisibility } from "../src/daemon/session-records.ts"
import {
  CreateSessionRequest,
  DaemonSession,
  SessionTurnMessage,
  sessionTurnMessageCoversSequence,
} from "../src/schema.ts"

test("DaemonSession defaults legacy records to app-visible sessions", () => {
  const session = DaemonSession.parse({
    acpSessionId: "acp-session-1",
    status: "active",
    agentName: "Codex",
    cwd: "/repo",
    lastSessionActivityAt: 1,
    mcpServers: [],
    configOptions: [],
    contextUsage: null,
  })

  expect(session.origin).toBe("app")
  expect(session.visibility).toBe("visible")
})

test("CreateSessionRequest accepts explicit provenance and visibility", () => {
  const request = CreateSessionRequest.parse({
    cwd: "/repo",
    mcpServers: [],
    origin: "pipeline",
    visibility: "hidden",
  })

  expect(request.origin).toBe("pipeline")
  expect(request.visibility).toBe("hidden")
})

test("session creation defaults untagged new sessions to sdk-hidden", () => {
  expect(
    resolveSessionOriginVisibility({
      existingSession: null,
      request: {},
    }),
  ).toEqual({
    origin: "sdk",
    visibility: "hidden",
  })
})

test("session creation defaults app sessions to visible", () => {
  expect(
    resolveSessionOriginVisibility({
      existingSession: null,
      request: { origin: "app" },
    }),
  ).toEqual({
    origin: "app",
    visibility: "visible",
  })
})

test("session reconnection preserves existing provenance and visibility", () => {
  expect(
    resolveSessionOriginVisibility({
      existingSession: {
        origin: "pipeline",
        visibility: "hidden",
      },
      request: {},
    }),
  ).toEqual({
    origin: "pipeline",
    visibility: "hidden",
  })
})

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
