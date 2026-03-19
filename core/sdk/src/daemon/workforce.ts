import type { DaemonIpcClient } from "@goddard-ai/daemon-client"
import { createDaemonIpcClient, createDaemonIpcClientFromEnv } from "@goddard-ai/daemon-client"
import type { DaemonWorkforce, DaemonWorkforceStatus } from "@goddard-ai/schema/daemon"
import type { WorkforceRequestIntent } from "@goddard-ai/schema/workforce"
import type { RunAgentOptions } from "./session/client.js"

// Configuration used to connect workforce helpers to a daemon IPC client.
export type WorkforceClientOptions = RunAgentOptions

function resolveDaemonClient(options?: WorkforceClientOptions): DaemonIpcClient {
  if (options?.client) {
    return options.client
  }

  if (options?.daemonUrl) {
    return createDaemonIpcClient({
      daemonUrl: options.daemonUrl,
      createClient: options.createClient,
    })
  }

  return createDaemonIpcClientFromEnv({
    env: options?.env,
    createClient: options?.createClient,
  }).client
}

export async function startDaemonWorkforce(
  rootDir: string,
  options?: WorkforceClientOptions,
): Promise<DaemonWorkforce> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceStart", { rootDir })
  return response.workforce
}

export async function getDaemonWorkforce(
  rootDir: string,
  options?: WorkforceClientOptions,
): Promise<DaemonWorkforce> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceGet", { rootDir })
  return response.workforce
}

export async function listDaemonWorkforces(
  options?: WorkforceClientOptions,
): Promise<DaemonWorkforceStatus[]> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceList", {})
  return response.workforces
}

export async function shutdownDaemonWorkforce(
  rootDir: string,
  options?: WorkforceClientOptions,
): Promise<boolean> {
  const client = resolveDaemonClient(options)
  const response = await client.send("workforceShutdown", { rootDir })
  return response.success
}

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
