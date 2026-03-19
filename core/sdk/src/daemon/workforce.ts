import type { DaemonWorkforce, DaemonWorkforceStatus } from "@goddard-ai/schema/daemon"
import type { WorkforceRequestIntent } from "@goddard-ai/schema/workforce"
import { resolveDaemonClient, type DaemonClientOptions } from "./client.js"

/** Shared daemon connection options for workforce lifecycle helpers. */
// Shared daemon client resolution options used by workforce helpers.
export type WorkforceClientOptions = DaemonClientOptions

/** Starts or reuses the daemon-managed workforce runtime for a repository root. */
export async function startDaemonWorkforce(
  rootDir: string,
  options?: WorkforceClientOptions,
): Promise<DaemonWorkforce> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceStart", { rootDir })
  return response.workforce
}

/** Returns the daemon-managed workforce state for a repository root. */
export async function getDaemonWorkforce(
  rootDir: string,
  options?: WorkforceClientOptions,
): Promise<DaemonWorkforce> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceGet", { rootDir })
  return response.workforce
}

/** Lists all workforce runtimes currently known to the daemon. */
export async function listDaemonWorkforces(
  options?: WorkforceClientOptions,
): Promise<DaemonWorkforceStatus[]> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceList", {})
  return response.workforces
}

/** Shuts down the daemon-managed workforce runtime for a repository root. */
export async function shutdownDaemonWorkforce(
  rootDir: string,
  options?: WorkforceClientOptions,
): Promise<boolean> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceShutdown", { rootDir })
  return response.success
}

/** Creates a new daemon-managed workforce request. */
export async function createDaemonWorkforceRequest(
  input: {
    rootDir: string
    targetAgentId: string
    message: string
    intent?: WorkforceRequestIntent
  },
  options?: WorkforceClientOptions,
): Promise<{ workforce: DaemonWorkforceStatus; requestId: string | null }> {
  const client = resolveDaemonClient(options)
  return client.send("workforceRequest", {
    rootDir: input.rootDir,
    targetAgentId: input.targetAgentId,
    input: input.message,
    intent: input.intent,
  })
}

/** Updates an existing daemon-managed workforce request. */
export async function updateDaemonWorkforceRequest(
  input: {
    rootDir: string
    requestId: string
    message: string
  },
  options?: WorkforceClientOptions,
): Promise<{ workforce: DaemonWorkforceStatus; requestId: string | null }> {
  const client = resolveDaemonClient(options)
  return client.send("workforceUpdate", {
    rootDir: input.rootDir,
    requestId: input.requestId,
    input: input.message,
  })
}

/** Cancels an existing daemon-managed workforce request. */
export async function cancelDaemonWorkforceRequest(
  input: {
    rootDir: string
    requestId: string
    reason?: string
  },
  options?: WorkforceClientOptions,
): Promise<{ workforce: DaemonWorkforceStatus; requestId: string | null }> {
  const client = resolveDaemonClient(options)
  return client.send("workforceCancel", input)
}

/** Truncates pending workforce work for a repository root or specific agent. */
export async function truncateDaemonWorkforce(
  input: {
    rootDir: string
    agentId?: string
    reason?: string
  },
  options?: WorkforceClientOptions,
): Promise<{ workforce: DaemonWorkforceStatus; requestId: string | null }> {
  const client = resolveDaemonClient(options)
  return client.send("workforceTruncate", input)
}
