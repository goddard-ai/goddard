import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import type {
  DaemonSession,
  GetSessionHistoryRequest,
  GetSessionHistoryResponse,
} from "@goddard-ai/session/schema"
import * as acp from "acp-client/protocol"

/** Minimal ACP client surface used by daemon-backed SDK sessions. */
export type AgentSessionAcpClient = {
  prompt(params: acp.PromptRequest): Promise<acp.PromptResponse>
  unstable_setSessionModel(params: acp.SetSessionModelRequest): Promise<acp.SetSessionModelResponse>
}

/** Managed agent session connected to the daemon over IPC. */
export class AgentSession {
  public readonly sessionId: DaemonSession["id"]

  private readonly acpSessionId: string
  private readonly acpClient: AgentSessionAcpClient
  private readonly daemonClient: DaemonIpcClient
  private readonly closeStream: () => Promise<void> | void

  constructor(
    sessionId: DaemonSession["id"],
    acpSessionId: string,
    acpClient: AgentSessionAcpClient,
    daemonClient: DaemonIpcClient,
    closeStream: () => Promise<void> | void,
  ) {
    this.sessionId = sessionId
    this.acpSessionId = acpSessionId
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
    return this.daemonClient.session.cancel({ id: this.sessionId })
  }

  /** Cancels the active turn and replaces it with one new prompt once the daemon observes a safe boundary. */
  async steer(userPrompt: string | acp.ContentBlock[]) {
    return this.daemonClient.session.steer({
      id: this.sessionId,
      prompt: typeof userPrompt === "string" ? userPrompt : [...userPrompt],
    })
  }

  /** Sets the active model for the connected agent session. */
  async setAgentModel(modelId: string) {
    await this.acpClient.unstable_setSessionModel({
      sessionId: this.acpSessionId,
      modelId,
    })
  }

  /** Retrieves one page of turn history for the connected agent session. */
  async getHistoryPage(
    input: Omit<GetSessionHistoryRequest, "id"> = {},
  ): Promise<GetSessionHistoryResponse> {
    return this.daemonClient.session.history({
      id: this.sessionId,
      ...input,
    })
  }

  /** Shuts down the connected agent session on the daemon. */
  async stop() {
    await this.closeStream()
    await this.daemonClient.session.shutdown({ id: this.sessionId }).catch(() => {})
  }
}
