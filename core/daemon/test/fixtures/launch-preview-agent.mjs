#!/usr/bin/env node
import { randomUUID } from "node:crypto"
import { Readable, Writable } from "node:stream"
import * as acp from "@agentclientprotocol/sdk"

class LaunchPreviewFixtureAgent {
  constructor(connection) {
    this.connection = connection
    this.sessions = new Map()
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
    const session = {
      currentModelId: "gpt-5.4",
      thinkingLevel: "medium",
    }
    this.sessions.set(sessionId, session)

    await this.connection.sessionUpdate({
      sessionId,
      update: {
        sessionUpdate: "available_commands_update",
        availableCommands: [
          {
            name: "plan",
            description: "Create or revise the plan",
            input: { hint: "What should change?" },
          },
          {
            name: "summarize",
            description: "Summarize the current progress",
          },
        ],
      },
    })

    return {
      sessionId,
      models: {
        currentModelId: session.currentModelId,
        availableModels: [
          {
            modelId: "gpt-5.4",
            name: "GPT-5.4",
            description: "Balanced frontier model",
          },
          {
            modelId: "gpt-5.4-mini",
            name: "GPT-5.4 Mini",
            description: "Faster lower-latency variant",
          },
        ],
      },
      configOptions: [
        {
          id: "thinking",
          type: "select",
          name: "Thinking level",
          category: "thought_level",
          description: "Select how much reasoning budget to use.",
          currentValue: session.thinkingLevel,
          options: [
            { value: "low", name: "Low", description: "Keep reasoning light." },
            {
              value: "medium",
              name: "Medium",
              description: "Balanced reasoning.",
            },
            {
              value: "high",
              name: "High",
              description: "Use the deepest reasoning.",
            },
          ],
        },
      ],
    }
  }

  async unstable_closeSession(params) {
    this.sessions.delete(params.sessionId)
    return {}
  }

  async unstable_setSessionModel(params) {
    const session = this.sessions.get(params.sessionId)
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`)
    }

    session.currentModelId = params.modelId
    return {}
  }

  async setSessionConfigOption(params) {
    const session = this.sessions.get(params.sessionId)
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`)
    }

    if (params.configId !== "thinking" || typeof params.value !== "string") {
      throw new Error("Unsupported config option")
    }

    session.thinkingLevel = params.value

    return {
      configOptions: [
        {
          id: "thinking",
          type: "select",
          name: "Thinking level",
          category: "thought_level",
          description: "Select how much reasoning budget to use.",
          currentValue: session.thinkingLevel,
          options: [
            { value: "low", name: "Low", description: "Keep reasoning light." },
            {
              value: "medium",
              name: "Medium",
              description: "Balanced reasoning.",
            },
            {
              value: "high",
              name: "High",
              description: "Use the deepest reasoning.",
            },
          ],
        },
      ],
    }
  }

  async prompt(params) {
    const session = this.sessions.get(params.sessionId)
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`)
    }

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: {
          type: "text",
          text: `model=${session.currentModelId};thinking=${session.thinkingLevel}`,
        },
      },
    })

    return {
      stopReason: "end_turn",
    }
  }

  async cancel() {}
}

const input = Writable.toWeb(process.stdout)
const output = Readable.toWeb(process.stdin)
const stream = acp.ndJsonStream(input, output)

new acp.AgentSideConnection((connection) => new LaunchPreviewFixtureAgent(connection), stream)
