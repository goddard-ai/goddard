import { randomBytes, randomUUID } from "node:crypto"
import treeKill from "@alloc/tree-kill"
import type { AgentService } from "@goddard-ai/agent/daemon"
import type { DaemonAgentEnvironmentService, DaemonConfigProvider } from "@goddard-ai/daemon-plugin"
import { git } from "@goddard-ai/libgit2"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AgentsConfig } from "@goddard-ai/schema/config"
import { createAcpClient } from "acp-client"
import * as acp from "acp-client/protocol"

import {
  SessionErrorCodes,
  type SessionLaunchPreviewRequest,
  type SessionLaunchPreviewResponse,
  type SessionsConfig,
} from "../schema.ts"
import { spawnAgentProcess, waitForAgentProcessExit } from "./agent-process.ts"
import {
  getSlashComposerSuggestions,
  MAX_COMPOSER_SUGGESTION_LIMIT,
} from "./composer-suggestions.ts"
import { checkoutBranch } from "./git/checkout.ts"
import { listLocalBranches } from "./git/refs.ts"
import { createSessionIpcError } from "./ipc-error.ts"
import { createLaunchLeaseKey, type LaunchLease } from "./launch-lease.ts"
import type { ActiveSession, SessionMemory } from "./session-memory.ts"
import { resolveGitRepoRoot, resolveGitWorktreeSource } from "./worktree.ts"

type LaunchPreparationRootConfig = {
  agents?: AgentsConfig
  sessions?: SessionsConfig
  registry?: Record<string, AgentDistribution>
}

type LaunchLeaseStore = {
  findByKey(key: string): LaunchLease | null
  reactivate(lease: LaunchLease): void
  register(lease: LaunchLease): void
}

/** Lists local git branches for one launch dialog in git's refname order. */
export async function listLaunchBranches(cwd: string) {
  const source = await resolveGitWorktreeSource(cwd)

  if (!source) {
    return { branches: [], currentBranch: null }
  }

  return await listLocalBranches(source.path)
}

/** Returns true when branch switching in the requested local checkout would risk user work. */
export async function inspectLaunchCheckoutDirty(cwd: string): Promise<boolean> {
  const repoRoot = await resolveGitRepoRoot(cwd)

  if (!repoRoot) {
    return false
  }

  const status = await git.status.getWorkingTreeStatus(repoRoot)
  return status.entries.length > 0
}

/** Switches the user's local checkout before launching the first prompt. */
export async function checkoutLocalBranch(params: { cwd: string; branchName: string }) {
  const repoRoot = await resolveGitRepoRoot(params.cwd)

  if (!repoRoot) {
    throw createSessionIpcError(SessionErrorCodes.LaunchOutsideRepository, { cwd: params.cwd })
  }

  if (await inspectLaunchCheckoutDirty(repoRoot)) {
    throw createSessionIpcError(SessionErrorCodes.LaunchDirtyCheckout, {
      cwd: params.cwd,
      repoRoot,
    })
  }

  const result = await checkoutBranch(repoRoot, params.branchName)

  if (result.status !== 0) {
    throw createSessionIpcError(SessionErrorCodes.LaunchCheckoutFailed, {
      branchName: params.branchName,
      cwd: params.cwd,
      repoRoot,
    })
  }
}

/** Owns launch-dialog preparation and ACP lease warming before durable session creation. */
export function createLaunchPreparationFeature({
  memory,
  launchLeaseStore,
  configProvider,
  getDaemonUrl,
  createAgentEnvironment,
  agentService,
  getPackageVersion,
  handlePermissionRequest,
  handleSessionUpdate,
}: {
  memory: SessionMemory
  launchLeaseStore: LaunchLeaseStore
  configProvider: DaemonConfigProvider<LaunchPreparationRootConfig>
  getDaemonUrl: () => string
  createAgentEnvironment: DaemonAgentEnvironmentService["createAgentEnvironment"]
  agentService: AgentService
  getPackageVersion: () => string
  handlePermissionRequest: (
    active: ActiveSession,
    params: acp.RequestPermissionRequest,
  ) => Promise<acp.RequestPermissionResponse>
  handleSessionUpdate: (active: ActiveSession, params: acp.SessionNotification) => Promise<void>
}) {
  async function getLaunchPreview(
    params: SessionLaunchPreviewRequest,
  ): Promise<SessionLaunchPreviewResponse> {
    const key = createLaunchLeaseKey(params)
    const [source, launchBranches, dirty] = await Promise.all([
      resolveGitWorktreeSource(params.cwd),
      listLaunchBranches(params.cwd),
      inspectLaunchCheckoutDirty(params.cwd),
    ])
    const repoRoot = source?.path ?? null
    const bare = source?.bare ?? false
    const { branches, currentBranch } = launchBranches
    const existingLease = launchLeaseStore.findByKey(key)
    if (existingLease) {
      launchLeaseStore.reactivate(existingLease)
      existingLease.repoRoot = repoRoot
      existingLease.branches = branches
      existingLease.currentBranch = currentBranch
      existingLease.dirty = dirty
      return {
        launchLeaseId: existingLease.id,
        repoRoot,
        bare,
        branches,
        currentBranch,
        dirty,
        configOptions: existingLease.configOptions,
        slashCommands: getSlashComposerSuggestions(
          existingLease.availableCommands,
          "",
          MAX_COMPOSER_SUGGESTION_LIMIT,
        ),
      }
    }

    const resolvedConfig = await configProvider
      .getRootConfig(params.cwd)
      .then((root) => root.config)
    const resolvedRegistry = resolvedConfig?.registry
    const launchLeaseId = randomUUID()
    const token = randomBytes(32).toString("hex")
    const agentProcess = await spawnAgentProcess({
      daemonUrl: getDaemonUrl(),
      token,
      agent: params.agent,
      cwd: params.cwd,
      createAgentEnvironment,
      envPolicy: resolvedConfig?.sessions?.envPolicy,
      agentService,
      registry: resolvedRegistry,
      managedAgents: resolvedConfig?.agents?.managed,
    })
    let availableCommands: acp.AvailableCommand[] = []
    let acpSessionId: string | null = null
    let client: Awaited<ReturnType<typeof createAcpClient>> | null = null
    let resolveAvailableCommands: (() => void) | null = null
    const history: acp.AnyMessage[] = []
    const availableCommandsReady = new Promise<void>((resolve) => {
      resolveAvailableCommands = resolve
    })

    try {
      client = await createAcpClient({
        stdin: agentProcess.stdin,
        stdout: agentProcess.stdout,
        clientInfo: {
          name: "npm:@goddard-ai/daemon",
          version: getPackageVersion(),
        },
        handler: {
          async requestPermission(permissionParams) {
            const active =
              memory.activeSessionsByAcpSessionId.get(permissionParams.sessionId) ??
              (acpSessionId
                ? (memory.activeSessionsByAcpSessionId.get(acpSessionId) ?? null)
                : null)
            if (active) {
              return await handlePermissionRequest(active, permissionParams)
            }

            return { outcome: { outcome: "cancelled" } }
          },
          async sessionUpdate(params) {
            const active =
              memory.activeSessionsByAcpSessionId.get(params.sessionId) ??
              (acpSessionId
                ? (memory.activeSessionsByAcpSessionId.get(acpSessionId) ?? null)
                : null)
            if (active) {
              await handleSessionUpdate(active, params)
              return
            }

            history.push({
              jsonrpc: "2.0",
              method: acp.CLIENT_METHODS.session_update,
              params,
            })
            if (params.update.sessionUpdate === "available_commands_update") {
              availableCommands = params.update.availableCommands
              resolveAvailableCommands?.()
              resolveAvailableCommands = null
            }
          },
        },
      })
      const session = await client.newSession({
        cwd: params.cwd,
        mcpServers: [],
      })
      acpSessionId = session.sessionId

      await Promise.race([
        availableCommandsReady,
        new Promise<void>((resolve) => {
          setTimeout(resolve, 120)
        }),
      ])

      const lease: LaunchLease = {
        id: launchLeaseId,
        key,
        agent: params.agent,
        cwd: params.cwd,
        token,
        acpSessionId,
        agentProcess,
        client,
        session,
        initializeResult: client.initialize,
        history,
        availableCommands,
        configOptions: session.configOptions ?? [],
        repoRoot,
        branches,
        currentBranch,
        dirty,
        releaseTimer: null,
        closing: null,
      }
      launchLeaseStore.register(lease)

      return {
        launchLeaseId: lease.id,
        repoRoot: lease.repoRoot,
        bare,
        branches: lease.branches,
        currentBranch: lease.currentBranch,
        dirty: lease.dirty,
        configOptions: lease.configOptions,
        slashCommands: getSlashComposerSuggestions(
          lease.availableCommands,
          "",
          MAX_COMPOSER_SUGGESTION_LIMIT,
        ),
      }
    } catch (error) {
      if (acpSessionId && client) {
        try {
          await client.closeSession({
            sessionId: acpSessionId,
          })
        } catch {
          // The launch lease failed before it could be returned to a caller.
        }
      }

      await client?.close().catch(() => {})
      await treeKill(agentProcess).catch(() => {})
      await waitForAgentProcessExit(agentProcess).catch(() => {})
      throw error
    }
  }

  return {
    getLaunchPreview,
  }
}
