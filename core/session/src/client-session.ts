import * as acp from "@agentclientprotocol/sdk"
import { SessionStorage } from "@goddard-ai/storage"
import { ChildProcess } from "node:child_process"

type ClosableSocket = {
  close: () => void
}

/**
 * Client-side handle for a running ACP agent session.
 */
export class AgentSession {
  /** Stable identifier for this ACP session instance. */
  public readonly sessionId: string

  /** Active ACP transport used for prompt/cancel RPCs. */
  private readonly acpClient: acp.ClientSideConnection

  /** Base HTTP address for session-scoped endpoints (history/shutdown). */
  private readonly serverAddress: string

  /** Underlying websocket connection backing the ACP transport. */
  private readonly ws: ClosableSocket

  /** Spawned local ACP server process, when this session owns one. */
  private readonly subprocess: ChildProcess | undefined

  constructor(
    sessionId: string,
    acpClient: acp.ClientSideConnection,
    serverAddress: string,
    ws: ClosableSocket,
    subprocess: ChildProcess | undefined,
  ) {
    this.sessionId = sessionId
    this.acpClient = acpClient
    this.serverAddress = serverAddress
    this.ws = ws
    this.subprocess = subprocess
  }

  /**
   * Sends a user prompt to the active session.
   *
   * Accepts either a plain-text string or pre-built ACP content blocks.
   */
  async prompt(userPrompt: string | acp.ContentBlock[]) {
    return this.acpClient.prompt({
      sessionId: this.sessionId,
      prompt: typeof userPrompt === "string" ? [{ type: "text", text: userPrompt }] : userPrompt,
    })
  }

  /** Requests cancellation of the currently running agent operation. */
  async cancel() {
    return this.acpClient.cancel({ sessionId: this.sessionId })
  }

  /**
   * Fetches current conversation history from the local session server.
   *
   * Returns an empty array if history cannot be retrieved.
   */
  async getHistory(): Promise<acp.AnyMessage[]> {
    const res = await fetch(`${this.serverAddress}/history`)
    if (res.ok) {
      return res.json()
    }
    return []
  }

  /**
   * Fully terminates the session connection and attempts server shutdown.
   *
   * If shutdown is unreachable, this still kills the owned subprocess.
   * If this client did not spawn the subprocess, it falls back to the
   * persisted session server PID and sends SIGTERM directly.
   */
  async stop() {
    this.ws.close()
    try {
      await fetch(`${this.serverAddress}/shutdown`, { method: "POST" })
    } catch {
      // no-op
    }

    if (this.subprocess) {
      this.subprocess.kill()
      return
    }

    const session = await SessionStorage.get(this.sessionId)
    if (typeof session?.serverPid === "number") {
      try {
        process.kill(session.serverPid, "SIGTERM")
      } catch {
        // no-op
      }
    }
  }
}
