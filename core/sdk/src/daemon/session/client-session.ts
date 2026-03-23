import * as acp from "@agentclientprotocol/sdk"
import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import type { DaemonSession } from "@goddard-ai/schema/daemon"

/** Managed agent session connected to the daemon over IPC. */
export class AgentSession {
  public readonly sessionId: string
  public readonly session: DaemonSession

  private readonly acpSessionId: string
  private readonly acpClient: acp.ClientSideConnection
  private readonly daemonClient: DaemonIpcClient
  private readonly closeStream: () => Promise<void> | void

  constructor(
    sessionId: string,
    acpSessionId: string,
    session: DaemonSession,
    acpClient: acp.ClientSideConnection,
    daemonClient: DaemonIpcClient,
    closeStream: () => Promise<void> | void,
  ) {
    this.sessionId = sessionId
    this.acpSessionId = acpSessionId
    this.session = session
    this.acpClient = acpClient
    this.daemonClient = daemonClient
    this.closeStream = closeStream
  }

  /** Sends a prompt to the connected agent session. */
  async prompt(userPrompt: string | acp.ContentBlock[]) {
    return this.acpClient.prompt({
      sessionId: this.acpSessionId,
      prompt: typeof userPrompt === "string" ? [{ type: "text", text: userPrompt }] : userPrompt,
    })
  }

  /** Cancels any currently pending agent work. */
  async cancel() {
    return this.acpClient.cancel({ sessionId: this.acpSessionId })
  }

  /** Retrieves the message history for the connected agent session. */
  async getHistory(): Promise<acp.AnyMessage[]> {
    const response = await this.daemonClient.send("sessionHistory", {
      id: this.sessionId,
    })
    return response.history
  }

  /** Shuts down the connected agent session on the daemon. */
  async stop() {
    await this.closeStream()
    await this.daemonClient.send("sessionShutdown", { id: this.sessionId }).catch(() => {})
  }
}
