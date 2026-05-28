import * as acp from "acp-client/protocol"

/** Starts a small agent-side ACP dispatcher for daemon integration fixtures. */
export function createFixtureAgentConnection(createAgent, stream) {
  const writer = stream.writable.getWriter()
  const connection = {
    sessionUpdate(params) {
      return writer.write({
        jsonrpc: "2.0",
        method: acp.CLIENT_METHODS.session_update,
        params,
      })
    },
  }
  const agent = createAgent(connection)

  void readFixtureAgentMessages(agent, writer, stream.readable)
}

/** Routes ACP client requests to the matching fixture agent method. */
async function readFixtureAgentMessages(agent, writer, readable) {
  const reader = readable.getReader()

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) {
        return
      }

      await handleFixtureAgentMessage(agent, writer, value)
    }
  } finally {
    reader.releaseLock()
  }
}

/** Handles one ACP request or notification from the daemon under test. */
async function handleFixtureAgentMessage(agent, writer, message) {
  if (!message || typeof message !== "object" || typeof message.method !== "string") {
    return
  }

  try {
    const result = await dispatchFixtureAgentMethod(agent, message.method, message.params)
    if ("id" in message) {
      await writer.write({
        jsonrpc: "2.0",
        id: message.id,
        result,
      })
    }
  } catch (error) {
    if ("id" in message) {
      await writer.write({
        jsonrpc: "2.0",
        id: message.id,
        error: {
          code: -32603,
          message: error instanceof Error ? error.message : String(error),
        },
      })
    }
  }
}

/** Keeps method-name conversion explicit for the ACP calls used by fixtures. */
function dispatchFixtureAgentMethod(agent, method, params) {
  switch (method) {
    case acp.AGENT_METHODS.initialize:
      return agent.initialize(params)
    case acp.AGENT_METHODS.session_new:
      return agent.newSession(params)
    case acp.AGENT_METHODS.session_prompt:
      return agent.prompt(params)
    case acp.AGENT_METHODS.session_cancel:
      return agent.cancel(params)
    case acp.AGENT_METHODS.session_set_mode:
      return agent.setSessionMode(params)
    case acp.AGENT_METHODS.session_close:
      return agent.closeSession(params)
    case acp.AGENT_METHODS.session_set_config_option:
      return agent.setSessionConfigOption(params)
    case acp.AGENT_METHODS.session_set_model:
      return agent.unstable_setSessionModel(params)
    case acp.AGENT_METHODS.authenticate:
      return agent.authenticate(params)
    default:
      throw new Error(`Unsupported fixture ACP method: ${method}`)
  }
}
