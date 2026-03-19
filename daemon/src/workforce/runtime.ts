import type { DaemonWorkforce, DaemonWorkforceStatus } from "@goddard-ai/schema/daemon"
import type {
  WorkforceAgentConfig,
  WorkforceConfig,
  WorkforceLedgerEvent,
  WorkforceProjection,
  WorkforceRequestIntent,
  WorkforceRequestRecord,
} from "@goddard-ai/schema/workforce"
import { randomUUID } from "node:crypto"
import { join } from "node:path"
import type { SessionManager } from "../session/index.ts"
import { ensureWorkforceFiles, readWorkforceConfig } from "./config.ts"
import {
  appendWorkforceLedgerEvent,
  applyWorkforceEvent,
  buildWorkforceQueues,
  readWorkforceLedger,
  replayWorkforceProjection,
  summarizeWorkforceProjection,
} from "./ledger.ts"
import { buildWorkforcePaths } from "./paths.ts"

/** Optional authenticated agent context derived from the calling daemon session. */
export interface WorkforceActorContext {
  sessionId: string | null
  agentId: string | null
  requestId: string | null
}

/** Input delivered to the daemon-owned session runner for one handled request. */
export interface WorkforceSessionRunInput {
  rootDir: string
  agent: WorkforceAgentConfig
  config: WorkforceConfig
  request: WorkforceRequestRecord
  recentActivity: WorkforceLedgerEvent[]
}

/** Session runner abstraction used by tests and the real daemon session bridge. */
export type WorkforceSessionRunner = (input: WorkforceSessionRunInput) => Promise<void>

/** Mutable runtime dependencies shared by one workload host. */
export interface WorkforceRuntimeDeps {
  sessionManager: SessionManager
  runSession?: WorkforceSessionRunner
}

function buildRecentActivity(
  events: WorkforceLedgerEvent[],
  request: WorkforceRequestRecord,
  limit: number = 12,
): WorkforceLedgerEvent[] {
  return events
    .filter((event) => {
      if ("requestId" in event && event.requestId === request.id) {
        return true
      }

      if ("agentId" in event && event.agentId === request.toAgentId) {
        return true
      }

      if ("toAgentId" in event && event.toAgentId === request.toAgentId) {
        return true
      }

      return false
    })
    .slice(-limit)
}

function formatRecentActivity(events: WorkforceLedgerEvent[]): string {
  if (events.length === 0) {
    return "No recent activity."
  }

  return events.map((event) => JSON.stringify(event)).join("\n")
}

function formatRequestContext(request: WorkforceRequestRecord): string {
  return [request.input, ...request.updates].filter((entry) => entry.trim().length > 0).join("\n\n")
}

function buildSystemPrompt(
  rootDir: string,
  config: WorkforceConfig,
  agent: WorkforceAgentConfig,
  request: WorkforceRequestRecord,
): string {
  const agentScope = agent.owns.join(", ")
  const rootRelativeCwd = agent.cwd === "." ? "." : agent.cwd

  return [
    `You are the workforce ${agent.role} agent "${agent.name}" (${agent.id}).`,
    `Repository root: ${rootDir}`,
    `Working directory: ${rootRelativeCwd}`,
    `Owned paths: ${agentScope}`,
    `Root agent id: ${config.rootAgentId}`,
    "You must not directly modify code outside your owned paths.",
    "Use the workforce executable to request delegated work, report a response, or suspend for escalation.",
    ...buildIntentSpecificSystemPrompt(config, agent, request.intent),
  ].join("\n")
}

function buildIntentSpecificSystemPrompt(
  config: WorkforceConfig,
  agent: WorkforceAgentConfig,
  intent: WorkforceRequestIntent,
): string[] {
  if (intent !== "create" || agent.id !== config.rootAgentId) {
    return []
  }

  return [
    "This request is a create request.",
    "You are being asked to create a new project from scratch or add new packages to the existing workspace when the requested feature needs them.",
    "Review the current workspace structure, packages, and ownership boundaries before deciding what to create.",
    "Do not assume new packages are required; first determine whether the requested feature fits the existing workspace.",
    "If new packages are appropriate, define them intentionally and delegate follow-up implementation work through the workforce.",
  ]
}

function buildInitialPrompt(
  rootDir: string,
  request: WorkforceRequestRecord,
  recentActivity: WorkforceLedgerEvent[],
): string {
  return [
    `Repository root: ${rootDir}`,
    `Current request id: ${request.id}`,
    request.fromAgentId ? `Sender agent id: ${request.fromAgentId}` : "Sender agent id: operator",
    `Request intent: ${request.intent}`,
    "",
    "Recent activity:",
    formatRecentActivity(recentActivity),
    "",
    "Request context:",
    formatRequestContext(request),
  ].join("\n")
}

async function defaultRunWorkforceSession(
  deps: WorkforceRuntimeDeps,
  input: WorkforceSessionRunInput,
): Promise<void> {
  const agentDistribution = input.agent.agent ?? input.config.defaultAgent
  const metadata = {
    workforce: {
      rootDir: input.rootDir,
      agentId: input.agent.id,
      requestId: input.request.id,
    },
  }

  await deps.sessionManager.createSession({
    agent: agentDistribution,
    cwd: input.agent.cwd === "." ? input.rootDir : join(input.rootDir, input.agent.cwd),
    mcpServers: [],
    systemPrompt: buildSystemPrompt(input.rootDir, input.config, input.agent, input.request),
    metadata,
    oneShot: true,
    initialPrompt: buildInitialPrompt(input.rootDir, input.request, input.recentActivity),
    env: {
      GODDARD_WORKFORCE_ROOT_DIR: input.rootDir,
      GODDARD_WORKFORCE_AGENT_ID: input.agent.id,
      GODDARD_WORKFORCE_REQUEST_ID: input.request.id,
    },
  })
}

function assertRequestExists(
  projection: WorkforceProjection,
  requestId: string,
): WorkforceRequestRecord {
  const request = projection.requests[requestId]
  if (!request) {
    throw new Error(`Unknown workforce request: ${requestId}`)
  }

  return request
}

function assertAgentExists(config: WorkforceConfig, agentId: string): WorkforceAgentConfig {
  const agent = config.agents.find((entry) => entry.id === agentId)
  if (!agent) {
    throw new Error(`Unknown workforce agent: ${agentId}`)
  }

  return agent
}

function isTerminalRequest(request: WorkforceRequestRecord): boolean {
  return (
    request.status === "completed" || request.status === "cancelled" || request.status === "errored"
  )
}

/** A daemon-managed repo-local workforce runtime and its active queue state. */
export class WorkforceRuntime {
  readonly #config: WorkforceConfig
  readonly #deps: WorkforceRuntimeDeps
  readonly #events: WorkforceLedgerEvent[]
  readonly #paths: ReturnType<typeof buildWorkforcePaths>
  readonly #rootDir: string
  readonly #runningAgents = new Set<string>()
  readonly #stopped = { value: false }

  #projection: WorkforceProjection

  private constructor(input: {
    rootDir: string
    config: WorkforceConfig
    projection: WorkforceProjection
    events: WorkforceLedgerEvent[]
    deps: WorkforceRuntimeDeps
  }) {
    this.#rootDir = input.rootDir
    this.#config = input.config
    this.#projection = input.projection
    this.#events = input.events
    this.#deps = input.deps
    this.#paths = buildWorkforcePaths(input.rootDir)
  }

  static async start(rootDir: string, deps: WorkforceRuntimeDeps): Promise<WorkforceRuntime> {
    await ensureWorkforceFiles(rootDir)
    const [config, projection, events] = await Promise.all([
      readWorkforceConfig(rootDir),
      replayWorkforceProjection(rootDir),
      readWorkforceLedger(rootDir),
    ])

    const runtime = new WorkforceRuntime({
      rootDir,
      config,
      projection,
      events,
      deps,
    })

    runtime.scheduleDrainForAllAgents()
    return runtime
  }

  getWorkforce(): DaemonWorkforce {
    return {
      ...this.getStatus(),
      config: this.#config,
    }
  }

  getStatus(): DaemonWorkforceStatus {
    return {
      state: "running",
      rootDir: this.#rootDir,
      configPath: this.#paths.configPath,
      ledgerPath: this.#paths.ledgerPath,
      ...this.#projection.summary,
    }
  }

  async stop(): Promise<void> {
    this.#stopped.value = true
  }

  async createRequest(input: {
    targetAgentId: string
    payload: string
    intent?: WorkforceRequestIntent
    actor: WorkforceActorContext
  }): Promise<string> {
    assertAgentExists(this.#config, input.targetAgentId)
    if (
      (input.intent ?? "default") === "create" &&
      input.targetAgentId !== this.#config.rootAgentId
    ) {
      throw new Error("Create requests must target the root workforce agent")
    }

    const requestId = randomUUID()
    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "request",
      requestId,
      toAgentId: input.targetAgentId,
      fromAgentId: input.actor.agentId,
      intent: input.intent ?? "default",
      input: input.payload,
    })

    return requestId
  }

  async updateRequest(input: {
    requestId: string
    payload: string
    actor: WorkforceActorContext
  }): Promise<void> {
    const request = assertRequestExists(this.#projection, input.requestId)
    if (input.actor.agentId && input.actor.agentId !== this.#config.rootAgentId) {
      throw new Error("Only the root agent or an operator can update workforce requests")
    }
    if (isTerminalRequest(request)) {
      throw new Error(`Cannot update terminal workforce request ${input.requestId}`)
    }

    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "update",
      requestId: input.requestId,
      input: input.payload,
    })
  }

  async cancelRequest(input: {
    requestId: string
    reason: string | null
    actor: WorkforceActorContext
  }): Promise<void> {
    const request = assertRequestExists(this.#projection, input.requestId)
    if (input.actor.agentId && input.actor.agentId !== this.#config.rootAgentId) {
      throw new Error("Only the root agent or an operator can cancel workforce requests")
    }
    if (isTerminalRequest(request)) {
      throw new Error(`Cannot cancel terminal workforce request ${input.requestId}`)
    }

    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "cancel",
      requestId: input.requestId,
      reason: input.reason,
    })
  }

  async truncate(input: {
    agentId: string | null
    reason: string | null
    actor: WorkforceActorContext
  }): Promise<void> {
    if (input.actor.agentId && input.actor.agentId !== this.#config.rootAgentId) {
      throw new Error("Only the root agent or an operator can truncate workforce queues")
    }

    if (input.agentId !== null) {
      assertAgentExists(this.#config, input.agentId)
    }

    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "truncate",
      agentId: input.agentId,
      reason: input.reason,
    })
  }

  async respond(input: {
    requestId: string
    output: string
    actor: WorkforceActorContext
  }): Promise<void> {
    const request = assertRequestExists(this.#projection, input.requestId)
    if (input.actor.agentId !== request.toAgentId) {
      throw new Error(
        `Agent ${input.actor.agentId ?? "unknown"} cannot respond to ${input.requestId}`,
      )
    }
    if (request.status !== "active") {
      throw new Error(`Workforce request ${input.requestId} is not active`)
    }

    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "response",
      requestId: input.requestId,
      agentId: request.toAgentId,
      output: input.output,
    })
  }

  async suspend(input: {
    requestId: string
    reason: string
    actor: WorkforceActorContext
  }): Promise<void> {
    const request = assertRequestExists(this.#projection, input.requestId)
    if (input.actor.agentId !== request.toAgentId) {
      throw new Error(`Agent ${input.actor.agentId ?? "unknown"} cannot suspend ${input.requestId}`)
    }
    if (request.status !== "active") {
      throw new Error(`Workforce request ${input.requestId} is not active`)
    }

    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "suspend",
      requestId: input.requestId,
      agentId: request.toAgentId,
      reason: input.reason,
    })
  }

  private async appendEvent(event: WorkforceLedgerEvent): Promise<void> {
    await appendWorkforceLedgerEvent(this.#rootDir, event)
    this.#events.push(event)
    applyWorkforceEvent(this.#projection.requests, event)
    this.#projection = {
      requests: this.#projection.requests,
      queues: buildWorkforceQueues(this.#projection.requests),
      summary: summarizeWorkforceProjection(this.#projection.requests),
    }

    this.scheduleDrainForAllAgents()
  }

  private scheduleDrainForAllAgents(): void {
    for (const agent of this.#config.agents) {
      this.scheduleDrain(agent.id)
    }
  }

  private scheduleDrain(agentId: string): void {
    if (this.#stopped.value || this.#runningAgents.has(agentId)) {
      return
    }

    queueMicrotask(() => {
      void this.drainAgent(agentId)
    })
  }

  private async drainAgent(agentId: string): Promise<void> {
    if (this.#stopped.value || this.#runningAgents.has(agentId)) {
      return
    }

    this.#runningAgents.add(agentId)

    try {
      while (this.#stopped.value === false) {
        const nextRequestId = this.#projection.queues[agentId]?.[0]
        if (!nextRequestId) {
          return
        }

        await this.processRequest(agentId, nextRequestId)
      }
    } finally {
      this.#runningAgents.delete(agentId)
      if (this.#projection.queues[agentId]?.length) {
        this.scheduleDrain(agentId)
      }
    }
  }

  private async processRequest(agentId: string, requestId: string): Promise<void> {
    const agent = assertAgentExists(this.#config, agentId)
    const request = assertRequestExists(this.#projection, requestId)
    const attempt = request.attemptCount + 1

    await this.appendEvent({
      id: randomUUID(),
      at: new Date().toISOString(),
      type: "handle",
      requestId,
      agentId,
      attempt,
      sessionId: null,
    })

    try {
      const runSession =
        this.#deps.runSession ?? ((input) => defaultRunWorkforceSession(this.#deps, input))
      await runSession({
        rootDir: this.#rootDir,
        agent,
        config: this.#config,
        request: assertRequestExists(this.#projection, requestId),
        recentActivity: buildRecentActivity(this.#events, request),
      })
    } catch (error) {
      await this.handleAttemptFailure(requestId, agentId, attempt, error)
      return
    }

    const latestRequest = assertRequestExists(this.#projection, requestId)
    if (latestRequest.status === "active") {
      await this.handleAttemptFailure(
        requestId,
        agentId,
        attempt,
        new Error(`Workforce request ${requestId} completed without a response or suspend event`),
      )
    }
  }

  private async handleAttemptFailure(
    requestId: string,
    agentId: string,
    attempt: number,
    error: unknown,
  ): Promise<void> {
    if (attempt >= 3) {
      await this.appendEvent({
        id: randomUUID(),
        at: new Date().toISOString(),
        type: "error",
        requestId,
        agentId,
        message: error instanceof Error ? error.message : String(error),
      })
      return
    }

    const request = assertRequestExists(this.#projection, requestId)
    request.status = "queued"
    request.activeSessionId = null
    this.#projection = {
      requests: this.#projection.requests,
      queues: buildWorkforceQueues(this.#projection.requests),
      summary: summarizeWorkforceProjection(this.#projection.requests),
    }
  }
}
