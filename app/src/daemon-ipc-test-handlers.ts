import type { Handlers } from "@goddard-ai/ipc/server"
import type { GetDaemonSessionHistoryResponse } from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"

/**
 * Builds the app transport test's daemon IPC handler stub from one shared place.
 * The explicit Handlers return type keeps this list exhaustive with the schema.
 */
export function createDaemonSessionTestIpcHandlers(): Handlers<typeof daemonIpcSchema> {
  let nextSessionId = 0
  const workforceRoots = new Set<string>()
  const sessionHistory = new Map<
    string,
    { acpId: string; history: GetDaemonSessionHistoryResponse["history"] }
  >()

  function getSessionResponse(id: string) {
    const session = sessionHistory.get(id)
    if (!session) {
      throw new Error("Session not found")
    }

    return {
      session: {
        id,
        acpId: session.acpId,
        status: "active" as const,
        agentName: "test-agent",
        cwd: process.cwd(),
        metadata: {},
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: session.history.length > 0,
          activeDaemonSession: true,
        },
        diagnostics: {
          eventCount: 0,
          historyLength: session.history.length,
          lastEventAt: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        errorMessage: null,
        blockedReason: null,
        initiative: null,
        lastAgentMessage: null,
      },
    }
  }

  function getWorkforceStatus(rootDir: string) {
    return {
      state: "running" as const,
      rootDir,
      configPath: `${rootDir}/.goddard/workforce.json`,
      ledgerPath: `${rootDir}/.goddard/ledger.jsonl`,
      activeRequestCount: 0,
      queuedRequestCount: 0,
      suspendedRequestCount: 0,
      failedRequestCount: 0,
    }
  }

  function getWorkforceResponse(rootDir: string) {
    return {
      workforce: {
        ...getWorkforceStatus(rootDir),
        config: {
          version: 1 as const,
          defaultAgent: "pi-acp",
          rootAgentId: "root",
          agents: [
            {
              id: "root",
              name: "@repo/root",
              role: "root" as const,
              cwd: ".",
              owns: ["."],
            },
          ],
        },
      },
    }
  }

  function getLoopStatus(rootDir: string, loopName: string) {
    return {
      state: "running" as const,
      rootDir,
      loopName,
      promptModulePath: `${rootDir}/.goddard/loops/${loopName}/prompt.js`,
      startedAt: "2026-03-20T00:00:00.000Z",
      sessionId: "session-1",
      acpId: "acp-1",
      cycleCount: 0,
      lastPromptAt: null,
    }
  }

  function getLoopResponse(rootDir: string, loopName: string) {
    return {
      loop: {
        ...getLoopStatus(rootDir, loopName),
        session: {
          agent: "pi-acp" as const,
          cwd: rootDir,
          mcpServers: [],
          systemPrompt: "Keep responses short.",
        },
        rateLimits: {
          cycleDelay: "30s",
          maxOpsPerMinute: 4,
          maxCyclesBeforePause: 200,
        },
        retries: {
          maxAttempts: 1,
          initialDelayMs: 500,
          maxDelayMs: 5_000,
          backoffFactor: 2,
          jitterRatio: 0.2,
        },
      },
    }
  }

  return {
    health: async () => ({ ok: true }),
    prSubmit: async () => ({ number: 1, url: "https://github.com/example/repo/pull/1" }),
    prReply: async () => ({ success: true }),
    sessionCreate: async () => {
      const id = `daemon-session-${nextSessionId++}`
      const acpId = `acp-session-${nextSessionId}`
      sessionHistory.set(id, { acpId, history: [] })
      return getSessionResponse(id)
    },
    sessionGet: async ({ id }) => getSessionResponse(id),
    sessionConnect: async ({ id }) => getSessionResponse(id),
    sessionHistory: async ({ id }) => {
      const session = sessionHistory.get(id)
      if (!session) {
        throw new Error("Session not found")
      }
      return {
        id,
        acpId: session.acpId,
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: session.history.length > 0,
          activeDaemonSession: true,
        },
        history: [...session.history],
      }
    },
    sessionDiagnostics: async ({ id }) => {
      const session = sessionHistory.get(id)
      if (!session) {
        throw new Error("Session not found")
      }
      return {
        id,
        acpId: session.acpId,
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: session.history.length > 0,
          activeDaemonSession: true,
        },
        events: [],
      }
    },
    sessionShutdown: async ({ id }) => ({
      id,
      success: sessionHistory.delete(id),
    }),
    sessionSend: async ({ id, message }) => {
      const session = sessionHistory.get(id)
      if (!session) {
        throw new Error("Session not found")
      }
      session.history.push(message as GetDaemonSessionHistoryResponse["history"][number])
      if (
        typeof message === "object" &&
        message !== null &&
        "id" in message &&
        (typeof message.id === "string" || typeof message.id === "number")
      ) {
        session.history.push({
          jsonrpc: "2.0",
          id: message.id,
          result: { stopReason: "end_turn" },
        })
      }
      return { accepted: true }
    },
    sessionResolveToken: async () => ({ id: "daemon-session-0" }),
    loopStart: async ({ rootDir, loopName }) => getLoopResponse(rootDir, loopName),
    loopGet: async ({ rootDir, loopName }) => getLoopResponse(rootDir, loopName),
    loopList: async () => ({
      loops: [],
    }),
    loopShutdown: async ({ rootDir, loopName }) => ({
      rootDir,
      loopName,
      success: true,
    }),
    workforceStart: async ({ rootDir }) => {
      workforceRoots.add(rootDir)
      return getWorkforceResponse(rootDir)
    },
    workforceGet: async ({ rootDir }) => {
      workforceRoots.add(rootDir)
      return getWorkforceResponse(rootDir)
    },
    workforceList: async () => ({
      workforces: Array.from(workforceRoots)
        .sort()
        .map((rootDir) => getWorkforceStatus(rootDir)),
    }),
    workforceShutdown: async ({ rootDir }) => ({
      rootDir,
      success: workforceRoots.delete(rootDir),
    }),
    workforceRequest: async ({ rootDir }) => {
      workforceRoots.add(rootDir)
      return {
        workforce: getWorkforceStatus(rootDir),
        requestId: "test-workforce-request",
      }
    },
    workforceUpdate: async ({ rootDir, requestId }) => {
      workforceRoots.add(rootDir)
      return {
        workforce: getWorkforceStatus(rootDir),
        requestId,
      }
    },
    workforceCancel: async ({ rootDir, requestId }) => {
      workforceRoots.add(rootDir)
      return {
        workforce: getWorkforceStatus(rootDir),
        requestId,
      }
    },
    workforceTruncate: async ({ rootDir }) => {
      workforceRoots.add(rootDir)
      return {
        workforce: getWorkforceStatus(rootDir),
        requestId: null,
      }
    },
    workforceRespond: async ({ rootDir, requestId }) => {
      workforceRoots.add(rootDir)
      return {
        workforce: getWorkforceStatus(rootDir),
        requestId,
      }
    },
    workforceSuspend: async ({ rootDir, requestId }) => {
      workforceRoots.add(rootDir)
      return {
        workforce: getWorkforceStatus(rootDir),
        requestId,
      }
    },
  }
}
