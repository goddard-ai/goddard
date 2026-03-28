import type { WorkforceRequestIntent } from "@goddard-ai/schema/workforce"
export { type WorkforceClientOptions } from "../daemon/workforce.ts"
import {
  cancelDaemonWorkforceRequest,
  createDaemonWorkforceRequest,
  getDaemonWorkforce,
  listDaemonWorkforces,
  shutdownDaemonWorkforce,
  startDaemonWorkforce,
  truncateDaemonWorkforce,
  updateDaemonWorkforceRequest,
} from "../daemon/workforce.ts"
import { type NodeDaemonClientOptions, resolveNodeDaemonClient } from "./client.ts"

/** Starts the daemon-managed workforce runtime for one repository using Node defaults. */
export async function startWorkforce(rootDir: string, options?: NodeDaemonClientOptions) {
  return startDaemonWorkforce(rootDir, { client: resolveNodeDaemonClient(options) })
}

/** Returns the daemon-managed workforce state for one repository using Node defaults. */
export async function getWorkforce(rootDir: string, options?: NodeDaemonClientOptions) {
  return getDaemonWorkforce(rootDir, { client: resolveNodeDaemonClient(options) })
}

/** Lists all daemon-managed workforce runtimes currently known to the daemon. */
export async function listWorkforces(options?: NodeDaemonClientOptions) {
  return listDaemonWorkforces({ client: resolveNodeDaemonClient(options) })
}

/** Stops the daemon-managed workforce runtime for one repository using Node defaults. */
export async function stopWorkforce(rootDir: string, options?: NodeDaemonClientOptions) {
  return shutdownDaemonWorkforce(rootDir, { client: resolveNodeDaemonClient(options) })
}

/** Creates one new daemon-managed workforce request using Node defaults. */
export async function createWorkforceRequest(
  input: {
    rootDir: string
    targetAgentId: string
    message: string
    intent?: WorkforceRequestIntent
  },
  options?: NodeDaemonClientOptions,
) {
  return createDaemonWorkforceRequest(input, { client: resolveNodeDaemonClient(options) })
}

/** Updates one daemon-managed workforce request using Node defaults. */
export async function updateWorkforceRequest(
  input: {
    rootDir: string
    requestId: string
    message: string
  },
  options?: NodeDaemonClientOptions,
) {
  return updateDaemonWorkforceRequest(input, { client: resolveNodeDaemonClient(options) })
}

/** Cancels one daemon-managed workforce request using Node defaults. */
export async function cancelWorkforceRequest(
  input: {
    rootDir: string
    requestId: string
    reason?: string
  },
  options?: NodeDaemonClientOptions,
) {
  return cancelDaemonWorkforceRequest(input, { client: resolveNodeDaemonClient(options) })
}

/** Truncates pending daemon-managed workforce work using Node defaults. */
export async function truncateWorkforce(
  input: {
    rootDir: string
    agentId?: string
    reason?: string
  },
  options?: NodeDaemonClientOptions,
) {
  return truncateDaemonWorkforce(input, { client: resolveNodeDaemonClient(options) })
}
