#!/usr/bin/env node
import * as acp from "@agentclientprotocol/sdk"
import { randomUUID } from "node:crypto"
import { appendFile, mkdir, rename, rm, writeFile } from "node:fs/promises"
import { dirname, join } from "node:path"
import { Readable, Writable } from "node:stream"

function promptTextFromBlocks(blocks) {
  if (!Array.isArray(blocks)) {
    return ""
  }

  return blocks
    .map((block) => {
      if (typeof block !== "object" || block === null || block.type !== "text") {
        return ""
      }

      return typeof block.text === "string" ? block.text : ""
    })
    .filter(Boolean)
    .join("\n")
    .replace(/^<system-prompt[\s\S]*?<\/system-prompt>\n?/u, "")
}

async function applyAction(cwd, action) {
  const targetPath = typeof action.path === "string" ? join(cwd, action.path) : null

  if (action.type === "write" && targetPath && typeof action.content === "string") {
    await mkdir(dirname(targetPath), { recursive: true })
    await writeFile(targetPath, action.content, "utf-8")
    return
  }

  if (action.type === "append" && targetPath && typeof action.content === "string") {
    await mkdir(dirname(targetPath), { recursive: true })
    await appendFile(targetPath, action.content, "utf-8")
    return
  }

  if (action.type === "delete" && targetPath) {
    await rm(targetPath, { recursive: true, force: true })
    return
  }

  if (action.type === "rename" && targetPath && typeof action.nextPath === "string") {
    const nextPath = join(cwd, action.nextPath)
    await mkdir(dirname(nextPath), { recursive: true })
    await rename(targetPath, nextPath)
  }
}

class GitEditAgent {
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

  async newSession(params) {
    const sessionId = randomUUID()
    this.sessions.set(sessionId, { cwd: params.cwd })
    return { sessionId }
  }

  async authenticate() {
    return {}
  }

  async setSessionMode() {
    return {}
  }

  async prompt(params) {
    const session = this.sessions.get(params.sessionId)
    if (!session) {
      throw new Error(`Session ${params.sessionId} not found`)
    }

    const promptText = promptTextFromBlocks(params.prompt)
    const parsed = JSON.parse(promptText)
    const actions = Array.isArray(parsed?.actions) ? parsed.actions : parsed ? [parsed] : []

    for (const action of actions) {
      await applyAction(session.cwd, action)
    }

    await this.connection.sessionUpdate({
      sessionId: params.sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: {
          type: "text",
          text: `applied:${actions.length}`,
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

new acp.AgentSideConnection((connection) => new GitEditAgent(connection), stream)
