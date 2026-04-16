#!/usr/bin/env node
import { randomUUID } from "node:crypto"
import { createInterface } from "node:readline"

const sessionId = `queue-agent-session-${randomUUID()}`
let activePrompt = null

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`)
}

function sendTextUpdate(text) {
  send({
    jsonrpc: "2.0",
    method: "session/update",
    params: {
      sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: {
          type: "text",
          text,
        },
      },
    },
  })
}

function sendToolCall(toolCallId, title) {
  send({
    jsonrpc: "2.0",
    method: "session/update",
    params: {
      sessionId,
      update: {
        sessionUpdate: "tool_call",
        toolCallId,
        title,
        kind: "other",
        status: "in_progress",
      },
    },
  })
}

function sendToolCallUpdate(toolCallId, title) {
  send({
    jsonrpc: "2.0",
    method: "session/update",
    params: {
      sessionId,
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId,
        title,
        status: "failed",
      },
    },
  })
}

function resolveCancelBoundary(text) {
  if (text === "hold:tool-boundary") {
    return "tool_call"
  }

  if (text === "hold:update-boundary") {
    return "tool_call_update"
  }

  return "final_only"
}

function toPromptText(prompt) {
  if (!Array.isArray(prompt)) {
    return ""
  }

  const text = prompt
    .map((block) => {
      if (typeof block === "object" && block !== null && block.type === "text") {
        return typeof block.text === "string" ? block.text : ""
      }

      return ""
    })
    .filter(Boolean)
    .join("\n")

  return text.replace(/^<system-prompt[\s\S]*?<\/system-prompt>\n?/u, "")
}

function finishPrompt(prompt, stopReason) {
  if (!prompt) {
    return
  }

  sendTextUpdate(
    `${stopReason === "cancelled" ? "prompt_cancelled" : "prompt_finished"}:${prompt.text}`,
  )
  send({
    jsonrpc: "2.0",
    id: prompt.id,
    result: { stopReason },
  })
}

function schedulePromptCompletion(prompt) {
  const text = prompt.text
  if (text.startsWith("hold")) {
    return
  }

  const delayMs = text.startsWith("wait:") ? Number(text.slice("wait:".length)) : 5
  setTimeout(
    () => {
      if (!activePrompt || activePrompt.id !== prompt.id) {
        return
      }

      activePrompt = null
      finishPrompt(prompt, "end_turn")
    },
    Number.isFinite(delayMs) ? delayMs : 5,
  )
}

const rl = createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
})

rl.on("line", (line) => {
  const trimmed = line.trim()
  if (!trimmed) {
    return
  }

  const message = JSON.parse(trimmed)

  if (message.method === "initialize") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: message.params?.protocolVersion ?? "0",
        agentCapabilities: {
          loadSession: true,
        },
        agentInfo: {
          name: "queue-agent",
          version: "1.0.0",
        },
      },
    })
    return
  }

  if (typeof message.method === "string" && message.method.includes("session/new")) {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        sessionId,
      },
    })
    return
  }

  if (typeof message.method === "string" && message.method.includes("session/load")) {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {},
    })
    return
  }

  if (message.method === "session/prompt") {
    const promptText = toPromptText(message.params?.prompt)
    if (activePrompt) {
      send({
        jsonrpc: "2.0",
        id: message.id,
        error: {
          code: -32000,
          message: `prompt overlap: ${promptText}`,
        },
      })
      return
    }

    activePrompt = {
      id: message.id,
      text: promptText,
      toolCallId: `tool-${message.id}`,
      cancelBoundary: resolveCancelBoundary(promptText),
    }
    sendTextUpdate(`prompt_started:${promptText}`)
    if (activePrompt.cancelBoundary === "tool_call_update") {
      sendToolCall(activePrompt.toolCallId, `prepared_boundary:${promptText}`)
    }
    schedulePromptCompletion(activePrompt)
    return
  }

  if (message.method === "session/cancel") {
    if (!activePrompt) {
      return
    }

    const cancelledPrompt = activePrompt
    activePrompt = null
    sendTextUpdate(`cancel_notice:${cancelledPrompt.text}`)
    if (cancelledPrompt.cancelBoundary === "tool_call") {
      setTimeout(() => {
        sendToolCall(cancelledPrompt.toolCallId, `cancel_boundary:${cancelledPrompt.text}`)
      }, 5)
    }
    if (cancelledPrompt.cancelBoundary === "tool_call_update") {
      setTimeout(() => {
        sendToolCallUpdate(cancelledPrompt.toolCallId, `cancel_boundary:${cancelledPrompt.text}`)
      }, 5)
    }
    setTimeout(() => {
      finishPrompt(cancelledPrompt, "cancelled")
    }, 20)
    return
  }

  if ("id" in message && message.id != null) {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {},
    })
  }
})
