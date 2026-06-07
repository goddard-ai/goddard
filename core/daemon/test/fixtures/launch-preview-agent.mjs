#!/usr/bin/env node
import { spawnSync } from "node:child_process"
import { randomUUID } from "node:crypto"
import { appendFileSync } from "node:fs"
import { Readable, Writable } from "node:stream"
import * as acp from "acp-client/protocol"

import { createFixtureAgentConnection } from "./acp-fixture-connection.mjs"

function recordEvent(event) {
  if (!process.env.LAUNCH_PREVIEW_AGENT_LOG) {
    return
  }

  appendFileSync(process.env.LAUNCH_PREVIEW_AGENT_LOG, `${JSON.stringify(event)}\n`)
}

function readCurrentBranch() {
  const result = spawnSync("git", ["branch", "--show-current"], {
    cwd: process.cwd(),
    encoding: "utf-8",
  })

  return result.status === 0 ? result.stdout.trim() : null
}

function createConfigOptions(session) {
  return [
    {
      id: "model",
      type: "select",
      name: "Model",
      category: "model",
      currentValue: session.currentModelId,
      options: [
        {
          value: "gpt-5.4",
          name: "GPT-5.4",
          description: "Balanced frontier model",
        },
        {
          value: "gpt-5.4-mini",
          name: "GPT-5.4 Mini",
          description: "Faster lower-latency variant",
        },
      ],
    },
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
  ]
}

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
    recordEvent({ type: "newSession", sessionId })

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
      configOptions: createConfigOptions(session),
    }
  }

  async closeSession(params) {
    recordEvent({ type: "closeSession", sessionId: params.sessionId })
    this.sessions.delete(params.sessionId)
    return {}
  }

  async setSessionConfigOption(params) {
    const session = this.sessions.get(params.sessionId)
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`)
    }

    if (typeof params.value !== "string") {
      throw new Error("Unsupported config option")
    }

    if (params.configId === "model") {
      session.currentModelId = params.value
      recordEvent({ type: "setModel", sessionId: params.sessionId, modelId: params.value })
    } else if (params.configId === "thinking") {
      session.thinkingLevel = params.value
    } else {
      throw new Error("Unsupported config option")
    }

    recordEvent({
      type: "setConfigOption",
      sessionId: params.sessionId,
      configId: params.configId,
      value: params.value,
    })

    return {
      configOptions: createConfigOptions(session),
    }
  }

  async prompt(params) {
    const session = this.sessions.get(params.sessionId)
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`)
    }
    recordEvent({
      type: "prompt",
      sessionId: params.sessionId,
      modelId: session.currentModelId,
      thinkingLevel: session.thinkingLevel,
      branchName: readCurrentBranch(),
    })

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

createFixtureAgentConnection((connection) => new LaunchPreviewFixtureAgent(connection), stream)
