import * as acp from "@agentclientprotocol/sdk"
import type {
  CreateDaemonSessionRequest,
  DaemonSession,
  DaemonSessionMetadata,
  GetDaemonSessionHistoryResponse,
} from "@goddard-ai/schema/daemon"
import type { SessionStatus } from "@goddard-ai/schema/db"
import type { AgentDistribution } from "@goddard-ai/schema/session-server"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { SessionStorage, type SQLSessionUpdate } from "@goddard-ai/storage"
import { randomUUID } from "node:crypto"
import { spawn, type ChildProcessByStdio } from "node:child_process"
import { join } from "node:path"
import { Readable, Writable } from "node:stream"
import { createAgentConnection, getAcpMessageResult, isAcpRequest, matchAcpRequest } from "./acp.ts"
import { fetchRegistryAgent } from "./registry.ts"

/** The current version of `@goddard-ai/daemon` */
declare const __VERSION__: string

function getPackageVersion(): string {
  try {
    return __VERSION__
  } catch {
    return "0.0.0"
  }
}

type ClientRequestMap = Map<string | number, acp.AnyMessage & { method: string }>

type PermissionRequest = acp.AnyMessage & {
  id: unknown
  params: acp.RequestPermissionRequest
}

type PromptRequestMessage = acp.AnyMessage & {
  params: acp.PromptRequest
}

type ActiveSession = {
  id: string
  acpId: string
  token: string
  process: ChildProcessByStdio<Writable, Readable, null>
  writer: WritableStreamDefaultWriter<acp.AnyMessage>
  subscription: {
    close: () => Promise<void>
  }
  status: SessionStatus
  history: acp.AnyMessage[]
  isFirstPrompt: boolean
  systemPrompt: string
  lastPermissionRequest: PermissionRequest | null
  clientRequests: ClientRequestMap
}

export type SessionManager = {
  createSession: (params: CreateDaemonSessionRequest) => Promise<DaemonSession>
  connectSession: (id: string) => Promise<DaemonSession>
  getSession: (id: string) => Promise<DaemonSession>
  getHistory: (id: string) => Promise<GetDaemonSessionHistoryResponse>
  sendMessage: (id: string, message: acp.AnyMessage) => Promise<void>
  shutdownSession: (id: string) => Promise<boolean>
  resolveSessionIdByToken: (token: string) => Promise<string>
  close: () => Promise<void>
}

export function injectSystemPrompt(
  request: acp.PromptRequest,
  systemPrompt: string,
): acp.PromptRequest {
  return {
    ...request,
    prompt: [
      { type: "text", text: `<system-prompt name="Goddard CLI">${systemPrompt}</system-prompt>` },
      ...request.prompt,
    ],
  }
}

function sessionStatusFromClientMessage(
  message: acp.AnyMessage,
  status: SessionStatus,
): SessionStatus | null {
  if (status !== "active") {
    return null
  }

  const cancelRequest = matchAcpRequest<acp.CancelRequestNotification>(
    message,
    acp.AGENT_METHODS.session_cancel,
  )
  if (cancelRequest) {
    return "cancelled"
  }

  return null
}

function sessionStatusFromAgentMessage(
  clientRequest: acp.AnyMessage | undefined,
  message: acp.AnyMessage,
): SessionStatus | null {
  const promptRequest = clientRequest
    ? matchAcpRequest<acp.PromptRequest>(clientRequest, acp.AGENT_METHODS.session_prompt)
    : null

  if (!promptRequest) {
    return null
  }

  const result = getAcpMessageResult<acp.PromptResponse>(message)
  if (result?.stopReason === "end_turn") {
    return "done"
  }

  return null
}

function isErrorSignal(signal: string | null): boolean {
  return signal === "SIGKILL" || signal === "SIGABRT" || signal === "SIGQUIT"
}

function shouldExitAfterInitialPrompt(params: CreateDaemonSessionRequest): boolean {
  return params.oneShot === true && params.initialPrompt !== undefined
}

function toIsoString(value: Date | number): string {
  return (value instanceof Date ? value : new Date(value)).toISOString()
}

function toDaemonSession(record: Awaited<ReturnType<typeof SessionStorage.get>>): DaemonSession {
  if (!record) {
    throw new Error("Session not found")
  }

  return {
    id: record.id,
    acpId: record.acpId,
    status: record.status,
    agentName: record.agentName,
    cwd: record.cwd,
    metadata: record.metadata,
    createdAt: toIsoString(record.createdAt),
    updatedAt: toIsoString(record.updatedAt),
    errorMessage: record.errorMessage,
    blockedReason: record.blockedReason,
    initiative: record.initiative,
    lastAgentMessage: record.lastAgentMessage,
  }
}

function agentNameFromInput(agent: string | AgentDistribution): string {
  if (typeof agent === "string") {
    return agent
  }

  return agent.package ?? agent.cmd ?? "custom"
}

function buildAgentProcessEnv(input: {
  daemonUrl: string
  token: string
  env?: Record<string, string>
}): NodeJS.ProcessEnv {
  const agentBinDir = join(import.meta.dirname, "../../agent-bin")
  const basePath = input.env?.PATH ?? process.env.PATH ?? ""

  return {
    ...process.env,
    ...input.env,
    PATH: basePath ? `${agentBinDir}:${basePath}` : agentBinDir,
    GODDARD_DAEMON_URL: input.daemonUrl,
    GODDARD_SESSION_TOKEN: input.token,
  }
}

async function spawnAgentProcess(
  daemonUrl: string,
  token: string,
  params: {
    agent: string | AgentDistribution
    cwd: string
    env?: Record<string, string>
  },
): Promise<ChildProcessByStdio<Writable, Readable, null>> {
  let agent = params.agent
  if (typeof agent === "string") {
    const fetchedAgent = await fetchRegistryAgent(agent)
    if (!fetchedAgent) {
      throw new Error(`Agent not found: ${agent}`)
    }
    agent = fetchedAgent.distribution
  }

  let cmd: string
  let args: string[]

  if (agent.type === "npx" && agent.package) {
    cmd = "npx"
    args = ["-y", agent.package]
  } else if (agent.type === "binary" && agent.cmd) {
    cmd = agent.cmd
    args = agent.args || []
  } else if (agent.type === "uvx" && agent.package) {
    cmd = "uvx"
    args = [agent.package]
  } else {
    throw new Error("Unsupported agent distribution")
  }

  return spawn(cmd, args, {
    stdio: ["pipe", "pipe", "inherit"],
    cwd: params.cwd,
    env: buildAgentProcessEnv({ daemonUrl, token, env: params.env }),
  })
}

async function initializeSession(
  input: Writable,
  output: Readable,
  params: CreateDaemonSessionRequest,
): Promise<{
  status: SessionStatus
  isFirstPrompt: boolean
  history: acp.AnyMessage[]
  acpId: string
}> {
  const history: acp.AnyMessage[] = []
  const stream = acp.ndJsonStream(
    Writable.toWeb(input),
    Readable.toWeb(output) as ReadableStream<Uint8Array>,
  )

  const agent = new acp.ClientSideConnection(
    () => ({
      async requestPermission() {
        return { outcome: { outcome: "cancelled" } }
      },
      async sessionUpdate(params: any) {
        history.push({
          jsonrpc: "2.0",
          method: acp.CLIENT_METHODS.session_update,
          params,
        } satisfies acp.AnyMessage)
      },
    }),
    stream,
  )

  try {
    await agent.initialize({
      protocolVersion: acp.PROTOCOL_VERSION,
      clientInfo: { name: "npm:@goddard-ai/daemon", version: getPackageVersion() },
    })

    const newSession = await agent.newSession(params)
    let status: SessionStatus = "active"
    let isFirstPrompt = true

    if (params.initialPrompt !== undefined) {
      const promptRequest = injectSystemPrompt(
        {
          sessionId: newSession.sessionId,
          prompt:
            typeof params.initialPrompt === "string"
              ? [{ type: "text", text: params.initialPrompt }]
              : params.initialPrompt,
        },
        params.systemPrompt,
      )

      history.push({
        jsonrpc: "2.0",
        method: acp.AGENT_METHODS.session_prompt,
        params: promptRequest,
      } satisfies acp.AnyMessage)

      const response = await agent.prompt(promptRequest)
      if (response.stopReason === "end_turn") {
        status = "done"
      }
      isFirstPrompt = false
    }

    return {
      status,
      isFirstPrompt,
      history,
      acpId: newSession.sessionId,
    }
  } finally {
    await stream.readable.cancel().catch(() => {})
    await stream.writable.close().catch(() => {})
  }
}

function parseRepoScope(metadata?: DaemonSessionMetadata): {
  owner: string
  repo: string
  allowedPrNumbers: number[]
} {
  const repository = metadata?.repository?.trim() ?? ""
  const [owner, repo] = repository.split("/")

  return {
    owner: owner ?? "",
    repo: repo ?? "",
    allowedPrNumbers: typeof metadata?.prNumber === "number" ? [metadata.prNumber] : [],
  }
}

export function createSessionManager(input: {
  daemonUrl: string
  publish: (id: string, message: acp.AnyMessage) => void
}): SessionManager {
  const activeSessions = new Map<string, ActiveSession>()
  const sessionHistory = new Map<string, { acpId: string; history: acp.AnyMessage[] }>()

  async function updateSession(id: string, update: SQLSessionUpdate): Promise<void> {
    const active = activeSessions.get(id)
    if (update.status && active) {
      active.status = update.status
    }

    await SessionStorage.update(id, update)
  }

  async function createSession(params: CreateDaemonSessionRequest): Promise<DaemonSession> {
    const id = randomUUID()
    const token = randomUUID()
    const scope = parseRepoScope(params.metadata)

    await SessionPermissionsStorage.create({
      sessionId: id,
      token,
      owner: scope.owner,
      repo: scope.repo,
      allowedPrNumbers: scope.allowedPrNumbers,
    })

    try {
      const process = await spawnAgentProcess(input.daemonUrl, token, {
        agent: params.agent,
        cwd: params.cwd,
        env: params.env,
      })

      const initialized = await initializeSession(process.stdin, process.stdout, params)

      sessionHistory.set(id, {
        acpId: initialized.acpId,
        history: [...initialized.history],
      })

      await SessionStorage.create({
        id,
        acpId: initialized.acpId,
        status: initialized.status,
        agentName: agentNameFromInput(params.agent),
        cwd: params.cwd,
        mcpServers: params.mcpServers,
        metadata: params.metadata ?? null,
      })

      if (shouldExitAfterInitialPrompt(params)) {
        await updateSession(id, { status: "done" })
        process.kill()
        await SessionPermissionsStorage.revoke(id).catch(() => {})
        return toDaemonSession(await SessionStorage.get(id))
      }

      const connection = createAgentConnection(process.stdin, process.stdout)
      const writer = connection.getWriter()
      const active: ActiveSession = {
        id,
        acpId: initialized.acpId,
        token,
        process,
        writer,
        subscription: { close: async () => {} },
        status: initialized.status,
        history: sessionHistory.get(id)?.history ?? [...initialized.history],
        isFirstPrompt: initialized.isFirstPrompt,
        systemPrompt: params.systemPrompt,
        lastPermissionRequest: null,
        clientRequests: new Map(),
      }

      active.subscription = connection.subscribe(async (message) => {
        if (isAcpRequest<PermissionRequest>(message, acp.CLIENT_METHODS.session_request_permission)) {
          active.lastPermissionRequest = message
        } else if ("id" in message && message.id != null) {
          const clientRequest = active.clientRequests.get(message.id)
          const nextStatus = sessionStatusFromAgentMessage(clientRequest, message)
          if (nextStatus) {
            await updateSession(active.id, { status: nextStatus })
          }
          if (clientRequest) {
            active.clientRequests.delete(message.id)
          }
        }

        active.history.push(message)
        input.publish(active.id, message)
      })

      const handleExit = async (code: number | null, signal: NodeJS.Signals | null) => {
        activeSessions.delete(active.id)
        await active.writer.close().catch(() => {})
        await active.subscription.close().catch(() => {})
        await SessionPermissionsStorage.revoke(active.id).catch(() => {})

        const nextUpdate: SQLSessionUpdate = {}
        if (code !== 0 && code !== null) {
          nextUpdate.status = "error"
          nextUpdate.errorMessage = `Exited with code ${code}`
        } else if (isErrorSignal(signal)) {
          nextUpdate.status = "error"
          nextUpdate.errorMessage = `Killed by ${signal}`
        } else if (active.status !== "done") {
          nextUpdate.status = "cancelled"
        }

        if (Object.keys(nextUpdate).length > 0) {
          await updateSession(active.id, nextUpdate).catch(() => {})
        }
      }

      process.once("exit", (code, signal) => {
        void handleExit(code, signal)
      })

      activeSessions.set(active.id, active)
      return toDaemonSession(await SessionStorage.get(id))
    } catch (error) {
      await SessionPermissionsStorage.revoke(id).catch(() => {})
      throw error
    }
  }

  async function getSession(id: string): Promise<DaemonSession> {
    return toDaemonSession(await SessionStorage.get(id))
  }

  async function connectSession(id: string): Promise<DaemonSession> {
    if (!activeSessions.has(id)) {
      throw new Error(`Session ${id} is not active`)
    }

    return getSession(id)
  }

  async function getHistory(id: string): Promise<GetDaemonSessionHistoryResponse> {
    const active = activeSessions.get(id)
    if (!active) {
      const session = await getSession(id)
      const archived = sessionHistory.get(id)
      return {
        id: session.id,
        acpId: session.acpId,
        history: archived ? [...archived.history] : [],
      }
    }

    return {
      id: active.id,
      acpId: active.acpId,
      history: [...active.history],
    }
  }

  async function sendMessage(id: string, message: acp.AnyMessage): Promise<void> {
    const active = activeSessions.get(id)
    if (!active) {
      throw new Error(`Session ${id} is not active`)
    }

    if (
      active.lastPermissionRequest &&
      "id" in message &&
      message.id === active.lastPermissionRequest.id
    ) {
      active.lastPermissionRequest = null
    } else {
      const nextStatus = sessionStatusFromClientMessage(message, active.status)
      if (nextStatus) {
        await updateSession(active.id, { status: nextStatus })
      }

      if (
        active.isFirstPrompt &&
        isAcpRequest<PromptRequestMessage>(message, acp.AGENT_METHODS.session_prompt)
      ) {
        active.isFirstPrompt = false
        message.params = injectSystemPrompt(message.params, active.systemPrompt)
      }
    }

    if ("id" in message && message.id != null && "method" in message) {
      active.clientRequests.set(message.id, message as acp.AnyMessage & { method: string })
    }

    active.history.push(message)
    input.publish(active.id, message)
    await active.writer.write(message)
  }

  async function shutdownSession(id: string): Promise<boolean> {
    const active = activeSessions.get(id)
    if (!active) {
      return false
    }

    active.process.kill()
    return true
  }

  async function resolveSessionIdByToken(token: string): Promise<string> {
    const record = await SessionPermissionsStorage.getByToken(token)
    if (!record) {
      throw new Error("Invalid session token")
    }

    return record.sessionId
  }

  async function close(): Promise<void> {
    for (const session of activeSessions.values()) {
      session.process.kill()
      await session.writer.close().catch(() => {})
      await session.subscription.close().catch(() => {})
      await SessionPermissionsStorage.revoke(session.id).catch(() => {})
    }
    activeSessions.clear()
    sessionHistory.clear()
  }

  return {
    createSession,
    connectSession,
    getSession,
    getHistory,
    sendMessage,
    shutdownSession,
    resolveSessionIdByToken,
    close,
  }
}
