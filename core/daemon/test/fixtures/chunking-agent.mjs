#!/usr/bin/env node
import * as acp from "@agentclientprotocol/sdk"
import { randomUUID } from "node:crypto"
import { Readable, Writable } from "node:stream"

class ChunkingFixtureAgent {
  constructor(connection) {
    this.connection = connection
    this.sessions = new Set()
  }

  async initialize() {
    return {
      protocolVersion: acp.PROTOCOL_VERSION,
      agentCapabilities: {
        loadSession: false,
      },
    }
  }

  async newSession() {
    const sessionId = randomUUID()
    this.sessions.add(sessionId)
    return { sessionId }
  }

  async authenticate() {
    return {}
  }

  async setSessionMode() {
    return {}
  }

  async prompt(params) {
    if (!this.sessions.has(params.sessionId)) {
      throw new Error(`Session ${params.sessionId} not found`)
    }

    for (const text of ["Chunked ", "response", "."]) {
      await this.connection.sessionUpdate({
        sessionId: params.sessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text,
          },
        },
      })
    }

    return {
      stopReason: "end_turn",
    }
  }

  async cancel() {}
}

const input = Writable.toWeb(process.stdout)
const output = Readable.toWeb(process.stdin)
const stream = acp.ndJsonStream(input, output)

new acp.AgentSideConnection((connection) => new ChunkingFixtureAgent(connection), stream)
