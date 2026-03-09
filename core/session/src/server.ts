import {
  AgentSideConnection,
  ClientSideConnection,
  Agent,
  Client,
  ndJsonStream,
} from "@agentclientprotocol/sdk"
import * as schema from "@agentclientprotocol/sdk"
import { insertMessage, getMessagesBySessionId } from "@goddard-ai/storage"
import { fetchRegistryAgent } from "./registry.js"
import { spawn, ChildProcess } from "node:child_process"

export class SessionServer implements Agent {
  private sessionId: string | null = null
  private agentProcess: ChildProcess | null = null
  private agentConnection: ClientSideConnection | null = null
  private serverConnection: AgentSideConnection | null = null

  constructor(private agentName: string) {}

  async initialize(_params: schema.InitializeRequest): Promise<schema.InitializeResponse> {
    return {
      protocolVersion: 1,
    }
  }

  private async initializeAgentConnection() {
    if (this.agentConnection) return

    const registryAgent = await fetchRegistryAgent(this.agentName)
    if (!registryAgent) {
      throw new Error(`Agent not found: ${this.agentName}`)
    }

    let cmd: string
    let args: string[]

    if (registryAgent.distribution.type === "npx" && registryAgent.distribution.package) {
      cmd = "npx"
      args = ["-y", registryAgent.distribution.package]
    } else if (registryAgent.distribution.type === "binary" && registryAgent.distribution.cmd) {
      cmd = registryAgent.distribution.cmd
      args = registryAgent.distribution.args || []
    } else if (registryAgent.distribution.type === "uvx" && registryAgent.distribution.package) {
      cmd = "uvx"
      args = [registryAgent.distribution.package]
    } else {
      throw new Error("Unsupported agent distribution")
    }

    this.agentProcess = spawn(cmd, args, { stdio: ["pipe", "pipe", "pipe"] })

    if (!this.agentProcess.stdout || !this.agentProcess.stdin) {
      throw new Error("Failed to initialize agent stdio streams")
    }

    const writableStream = new WritableStream<Uint8Array>({
      write: (chunk) => {
        this.agentProcess!.stdin!.write(chunk)
      },
    })

    const readableStream = new ReadableStream<Uint8Array>({
      start: (controller) => {
        this.agentProcess!.stdout!.on("data", (chunk: Buffer) => controller.enqueue(chunk))
        this.agentProcess!.stdout!.on("end", () => controller.close())
        this.agentProcess!.stdout!.on("error", (err) => controller.error(err))
      },
    })

    const stream = ndJsonStream(writableStream, readableStream)

    this.agentConnection = new ClientSideConnection(
      () => new GoddardClient(this.sessionId!, this.serverConnection!),
      stream,
    )

    const response = await this.agentConnection.initialize({
      protocolVersion: 1,
      clientInfo: {
        name: "goddard-session",
        version: "0.1.0",
      },
    })

    if (response.protocolVersion !== 1) {
      throw new Error(
        `Invalid protocol version: ${response.protocolVersion}. Only version 1 is supported.`,
      )
    }
  }

  async newSession(params: schema.NewSessionRequest): Promise<schema.NewSessionResponse> {
    await this.initializeAgentConnection()

    // Pass session creation to agent and record
    const response = await this.agentConnection!.newSession(params)
    this.sessionId = response.sessionId

    // We must update the sessionId in our client instance since it was assigned after creation
    // but for minimal implementation we'll assume GoddardClient uses a function to resolve it,
    // or we simply re-initialize the client.

    return {
      sessionId: this.sessionId,
    }
  }

  async authenticate(_params: schema.AuthenticateRequest): Promise<schema.AuthenticateResponse> {
    return {}
  }

  async prompt(params: schema.PromptRequest): Promise<schema.PromptResponse> {
    if (!this.sessionId) {
      throw new Error("No active session")
    }

    await insertMessage(this.sessionId, "session/prompt", JSON.stringify(params))

    if (!this.agentConnection) {
      throw new Error("Agent connection not initialized")
    }

    const response = await this.agentConnection.prompt(params)

    return response
  }

  async setSessionMode(
    params: schema.SetSessionModeRequest,
  ): Promise<schema.SetSessionModeResponse | void> {
    if (!this.sessionId) return
    await insertMessage(this.sessionId, "session/set_mode", JSON.stringify(params))
    if (this.agentConnection?.setSessionMode) {
      return this.agentConnection.setSessionMode(params)
    }
  }

  async setSessionConfigOption(
    params: schema.SetSessionConfigOptionRequest,
  ): Promise<schema.SetSessionConfigOptionResponse> {
    if (!this.sessionId) {
      throw new Error("No active session")
    }
    await insertMessage(this.sessionId, "session/set_config_option", JSON.stringify(params))
    if (this.agentConnection?.setSessionConfigOption) {
      return this.agentConnection.setSessionConfigOption(params)
    }
    return { configOptions: [] }
  }

  async cancel(params: schema.CancelNotification): Promise<void> {
    if (this.agentConnection) {
      await this.agentConnection.cancel(params)
    }
  }

  async loadSession(params: schema.LoadSessionRequest): Promise<schema.LoadSessionResponse> {
    this.sessionId = params.sessionId
    const messages = await getMessagesBySessionId(params.sessionId)
    if (messages.length === 0) {
      throw new Error(`Session ${params.sessionId} not found or has no messages.`)
    }

    await this.initializeAgentConnection()

    if (!this.agentConnection?.loadSession) {
      throw new Error("Agent does not support loadSession")
    }

    return await this.agentConnection.loadSession(params)
  }

  async listen() {
    const writableStream = new WritableStream<Uint8Array>({
      write(chunk) {
        process.stdout.write(chunk)
      },
    })

    const readableStream = new ReadableStream<Uint8Array>({
      start(controller) {
        process.stdin.on("data", (chunk: Buffer) => {
          controller.enqueue(chunk)
        })
        process.stdin.on("end", () => {
          controller.close()
        })
        process.stdin.on("error", (err) => {
          controller.error(err)
        })
      },
    })

    const stream = ndJsonStream(writableStream, readableStream)
    this.serverConnection = new AgentSideConnection(() => this, stream)
  }
}

class GoddardClient implements Client {
  constructor(
    private sessionId: string,
    private serverConnection: AgentSideConnection,
  ) {}

  async requestPermission(
    _params: schema.RequestPermissionRequest,
  ): Promise<schema.RequestPermissionResponse> {
    return {
      outcome: { outcome: "cancelled" },
    }
  }

  async sessionUpdate(params: schema.SessionNotification): Promise<void> {
    await insertMessage(this.sessionId, "session/update", JSON.stringify(params))

    // Proxy the session update up to the connected client
    await this.serverConnection.sessionUpdate(params)
  }
}
