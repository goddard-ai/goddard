import * as acp from "@agentclientprotocol/sdk"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createNodeClient } from "@goddard-ai/ipc"

export class AgentSession {
  public readonly sessionId: string

  private readonly acpSessionId: string
  private readonly acpClient: acp.ClientSideConnection
  private readonly daemonClient: ReturnType<typeof createNodeClient<typeof daemonIpcSchema>>
  private readonly closeStream: () => void

  constructor(
    sessionId: string,
    acpSessionId: string,
    acpClient: acp.ClientSideConnection,
    daemonClient: ReturnType<typeof createNodeClient<typeof daemonIpcSchema>>,
    closeStream: () => void,
  ) {
    this.sessionId = sessionId
    this.acpSessionId = acpSessionId
    this.acpClient = acpClient
    this.daemonClient = daemonClient
    this.closeStream = closeStream
  }

  async prompt(userPrompt: string | acp.ContentBlock[]) {
    return this.acpClient.prompt({
      sessionId: this.acpSessionId,
      prompt: typeof userPrompt === "string" ? [{ type: "text", text: userPrompt }] : userPrompt,
    })
  }

  async cancel() {
    return this.acpClient.cancel({ sessionId: this.acpSessionId })
  }

  async getHistory(): Promise<acp.AnyMessage[]> {
    const response = await this.daemonClient.send("sessionHistory", {
      id: this.sessionId,
    })
    return response.history
  }

  async stop() {
    this.closeStream()
    await this.daemonClient.send("sessionShutdown", { id: this.sessionId }).catch(() => {})
  }
}
