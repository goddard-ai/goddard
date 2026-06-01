/** Launch-lease bookkeeping for ACP sessions prepared before durable daemon session creation. */
import treeKill from "@alloc/tree-kill"
import type { DaemonLogger } from "@goddard-ai/daemon-plugin"
import type {
  CreateSessionRequest,
  SessionLaunchBranch,
  SessionLaunchPreviewRequest,
} from "@goddard-ai/schema/daemon"
import type { AcpClient, AcpSession } from "acp-client"
import type * as acp from "acp-client/protocol"
import { getErrorMessage } from "radashi"

import { waitForAgentProcessExit, type AgentProcessHandle } from "./agent-process.ts"
import type { ResolvedCreateSessionRequest } from "./session-records.ts"
import type { PreparedSessionWorktree } from "./worktree.ts"

const LAUNCH_LEASE_RELEASE_TIMEOUT_MS = 10 * 1000

/** Prepared ACP session kept alive while the launch dialog gathers final user choices. */
export type LaunchLease = {
  id: string
  key: string
  agent: NonNullable<SessionLaunchPreviewRequest["agent"]>
  cwd: string
  token: string
  acpSessionId: string
  agentProcess: AgentProcessHandle
  client: AcpClient
  session: AcpSession
  initializeResult: acp.InitializeResponse
  history: acp.AnyMessage[]
  availableCommands: acp.AvailableCommand[]
  models: acp.SessionModelState | null
  configOptions: acp.SessionConfigOption[]
  repoRoot: string | null
  branches: SessionLaunchBranch[]
  dirty: boolean
  releaseTimer: ReturnType<typeof setTimeout> | null
  closing: Promise<void> | null
}

/** Builds the reuse key for the ACP session prepared behind one launch-dialog option set. */
export function createLaunchLeaseKey(params: {
  agent: NonNullable<SessionLaunchPreviewRequest["agent"]>
  cwd: string
}) {
  return JSON.stringify([params.cwd, params.agent])
}

/** Compares launch agents structurally because config-resolved distributions may be object values. */
function isSameLaunchAgent(
  left: NonNullable<SessionLaunchPreviewRequest["agent"]>,
  right: NonNullable<CreateSessionRequest["agent"]>,
) {
  return JSON.stringify(left) === JSON.stringify(right)
}

/** Returns true when a launch lease was created with the same ACP-facing session inputs. */
function canPromoteLaunchLease(params: {
  lease: LaunchLease
  request: ResolvedCreateSessionRequest
  cwd: string
  existingSession: unknown | null
  worktree: PreparedSessionWorktree | null
}) {
  return (
    params.existingSession === null &&
    params.worktree === null &&
    params.request.worktree?.enabled !== true &&
    params.request.localCheckout === undefined &&
    params.request.mcpServers.length === 0 &&
    params.request.env === undefined &&
    params.request.metadata === undefined &&
    params.request.repository === undefined &&
    params.request.prNumber === undefined &&
    params.lease.cwd === params.cwd &&
    isSameLaunchAgent(params.lease.agent, params.request.agent)
  )
}

/** Creates the daemon-local registry that owns launch lease lookup, release, and cleanup. */
export function createLaunchLeaseStore(input: { logger: DaemonLogger }) {
  const launchLeases = new Map<string, LaunchLease>()
  const launchLeaseIdsByKey = new Map<string, string>()

  function remove(lease: LaunchLease) {
    launchLeases.delete(lease.id)
    if (launchLeaseIdsByKey.get(lease.key) === lease.id) {
      launchLeaseIdsByKey.delete(lease.key)
    }
  }

  function cancelReleaseTimer(lease: LaunchLease) {
    if (!lease.releaseTimer) {
      return
    }

    clearTimeout(lease.releaseTimer)
    lease.releaseTimer = null
  }

  async function close(lease: LaunchLease, reason: string) {
    if (lease.closing) {
      return lease.closing
    }

    cancelReleaseTimer(lease)
    remove(lease)
    lease.closing = (async () => {
      input.logger.log("launch_lease_closing", {
        launchLeaseId: lease.id,
        acpSessionId: lease.acpSessionId,
        reason,
      })
      await lease.client
        .closeSession({
          sessionId: lease.acpSessionId,
        })
        .catch(() => {})
      await lease.client.close().catch(() => {})
      await treeKill(lease.agentProcess).catch(() => {})
      await waitForAgentProcessExit(lease.agentProcess).catch(() => {})
    })()

    return lease.closing
  }

  function scheduleRelease(lease: LaunchLease, reason: string) {
    if (lease.closing || lease.releaseTimer) {
      return
    }

    input.logger.log("launch_lease_release_scheduled", {
      launchLeaseId: lease.id,
      acpSessionId: lease.acpSessionId,
      reason,
      timeoutMs: LAUNCH_LEASE_RELEASE_TIMEOUT_MS,
    })
    lease.releaseTimer = setTimeout(() => {
      void close(lease, reason).catch((error) => {
        input.logger.log("launch_lease_close_failed", {
          launchLeaseId: lease.id,
          reason,
          errorMessage: getErrorMessage(error),
        })
      })
    }, LAUNCH_LEASE_RELEASE_TIMEOUT_MS)
  }

  return {
    register(lease: LaunchLease) {
      launchLeases.set(lease.id, lease)
      launchLeaseIdsByKey.set(lease.key, lease.id)
      lease.agentProcess.onceExit(() => {
        if (launchLeases.get(lease.id) === lease) {
          cancelReleaseTimer(lease)
          remove(lease)
        }
      })
    },
    findByKey(key: string) {
      const id = launchLeaseIdsByKey.get(key)
      return id ? (launchLeases.get(id) ?? null) : null
    },
    reactivate(lease: LaunchLease) {
      cancelReleaseTimer(lease)
    },
    takeCompatible(params: {
      launchLeaseId: string | undefined
      request: ResolvedCreateSessionRequest
      cwd: string
      existingSession: unknown | null
      worktree: PreparedSessionWorktree | null
    }) {
      if (!params.launchLeaseId) {
        return null
      }

      const lease = launchLeases.get(params.launchLeaseId) ?? null
      if (!lease) {
        return null
      }

      if (!canPromoteLaunchLease({ ...params, lease })) {
        scheduleRelease(lease, "incompatible_launch")
        return null
      }

      cancelReleaseTimer(lease)
      remove(lease)
      return lease
    },
    scheduleReleaseById(launchLeaseId: string, reason: string) {
      const lease = launchLeases.get(launchLeaseId) ?? null
      if (!lease) {
        return false
      }

      scheduleRelease(lease, reason)
      return true
    },
    async closeAll(reason: string) {
      while (launchLeases.size > 0) {
        const lease = launchLeases.values().next().value
        if (!lease) {
          break
        }

        await close(lease, reason).catch(() => {})
      }
      launchLeases.clear()
      launchLeaseIdsByKey.clear()
    },
  }
}
