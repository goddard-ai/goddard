import type { DaemonConfigProvider, DaemonLogger } from "@goddard-ai/daemon-plugin"
import type { WorktreePlugin } from "@goddard-ai/worktree-plugin"
import { getErrorMessage } from "radashi"

import type { SessionDb } from "../daemon.ts"
import type {
  DaemonLaunchWorktree,
  PrepareSessionLaunchWorktreeRequest,
  PrepareSessionLaunchWorktreeResponse,
  ReleaseSessionLaunchWorktreeRequest,
  ReleaseSessionLaunchWorktreeResponse,
  WorktreesConfig,
} from "../schema.ts"
import { resolveAvailableWorktreeBranchName } from "./worktree-branch.ts"
import {
  cleanupSessionWorktree,
  resolveGitHeadRef,
  resolveGitWorktreeSource,
  toPreparedSessionWorktree,
  type PreparedSessionWorktree,
  type SessionWorktreeState,
} from "./worktree.ts"
import { prepareFreshWorktree } from "./worktrees/bootstrap.ts"
import { createWorktree } from "./worktrees/index.ts"
import { defaultPlugin } from "./worktrees/plugins/default.ts"

const LAUNCH_WORKTREE_RELEASE_TIMEOUT_MS = 15 * 60 * 1000

type LaunchWorktreeRootConfig = {
  worktrees?: WorktreesConfig
}

type WorktreePluginManager = {
  getPlugins(cwd: string): Promise<WorktreePlugin[]>
}

type LaunchWorktreeTimer = ReturnType<typeof setTimeout>

function createLaunchWorktreeKey(params: PrepareSessionLaunchWorktreeRequest) {
  return JSON.stringify([params.cwd, params.baseBranchName ?? null])
}

function toWorktreeState(record: DaemonLaunchWorktree): SessionWorktreeState {
  const { id: _id, key: _key, releaseAfter: _releaseAfter, ...worktree } = record
  return worktree
}

function toResponse(record: DaemonLaunchWorktree): PrepareSessionLaunchWorktreeResponse {
  const { id, key: _key, releaseAfter: _releaseAfter, ...worktree } = record
  return {
    launchWorktreeId: id,
    worktree,
  }
}

/** Owns launch-dialog worktrees prepared before durable session creation. */
export function createLaunchWorktreeFeature({
  db,
  configProvider,
  logger,
  worktreePluginManager,
}: {
  db: SessionDb
  configProvider: DaemonConfigProvider<LaunchWorktreeRootConfig>
  logger: DaemonLogger
  worktreePluginManager: WorktreePluginManager
}) {
  const releaseTimers = new Map<string, LaunchWorktreeTimer>()

  function cancelReleaseTimer(id: string) {
    const timer = releaseTimers.get(id)
    if (!timer) {
      return
    }

    clearTimeout(timer)
    releaseTimers.delete(id)
  }

  async function cleanupRecord(record: DaemonLaunchWorktree, reason: string) {
    cancelReleaseTimer(record.id)
    db.launchWorktrees.delete(record.id)
    const worktree = toWorktreeState(record)
    try {
      await cleanupSessionWorktree(worktree, {
        worktreePlugins: await worktreePluginManager.getPlugins(worktree.repoRoot),
      })
      logger.log("launch_worktree_cleaned", {
        launchWorktreeId: record.id,
        worktreeDir: record.worktreeDir,
        reason,
      })
    } catch (error) {
      logger.log("launch_worktree_cleanup_failed", {
        launchWorktreeId: record.id,
        worktreeDir: record.worktreeDir,
        reason,
        errorMessage: getErrorMessage(error),
      })
    }
  }

  function scheduleRelease(record: DaemonLaunchWorktree, reason: string) {
    if (releaseTimers.has(record.id)) {
      return
    }

    const releaseAfter = new Date(Date.now() + LAUNCH_WORKTREE_RELEASE_TIMEOUT_MS).toISOString()
    db.launchWorktrees.update(record.id, { releaseAfter })
    logger.log("launch_worktree_release_scheduled", {
      launchWorktreeId: record.id,
      worktreeDir: record.worktreeDir,
      reason,
      timeoutMs: LAUNCH_WORKTREE_RELEASE_TIMEOUT_MS,
    })
    releaseTimers.set(
      record.id,
      setTimeout(() => {
        const latest = db.launchWorktrees.get(record.id) ?? null
        if (!latest) {
          releaseTimers.delete(record.id)
          return
        }

        void cleanupRecord(latest, reason)
      }, LAUNCH_WORKTREE_RELEASE_TIMEOUT_MS),
    )
  }

  function reactivate(record: DaemonLaunchWorktree) {
    cancelReleaseTimer(record.id)
    if (record.releaseAfter !== null) {
      db.launchWorktrees.update(record.id, { releaseAfter: null })
    }
  }

  function findById(id: string) {
    return db.launchWorktrees.findMany().find((record) => record.id === id) ?? null
  }

  async function prepare(
    params: PrepareSessionLaunchWorktreeRequest,
  ): Promise<PrepareSessionLaunchWorktreeResponse> {
    const key = createLaunchWorktreeKey(params)
    const existing =
      db.launchWorktrees.first({
        where: { key },
      }) ?? null

    if (existing) {
      reactivate(existing)
      return toResponse({
        ...existing,
        releaseAfter: null,
      })
    }

    const source = await resolveGitWorktreeSource(params.cwd)
    if (!source) {
      return {
        launchWorktreeId: null,
        worktree: null,
      }
    }

    const [config, worktreePlugins] = await Promise.all([
      configProvider.getRootConfig(params.cwd).then((root) => root.config),
      worktreePluginManager.getPlugins(params.cwd),
    ])
    let worktree: SessionWorktreeState | null = null

    try {
      worktree = await createWorktree({
        cwd: source.path,
        requestedCwd: params.cwd,
        mergeTargetBranch: await resolveGitHeadRef(source.path).catch(() => null),
        branchName: await resolveAvailableWorktreeBranchName({
          cwd: source.path,
          branchPrefix: config?.worktrees?.branchPrefix,
        }),
        baseBranchName: params.baseBranchName,
        plugins: worktreePlugins,
        defaultPluginDirName: config?.worktrees?.defaultFolder,
      })

      if (worktree.poweredBy === defaultPlugin.name) {
        await prepareFreshWorktree({
          repoRoot: worktree.repoRoot,
          worktreeDir: worktree.worktreeDir,
          config: config?.worktrees?.bootstrap,
          onEvent: (event) => {
            logger.log("launch_worktree_bootstrap_event", {
              launchWorktreeKey: key,
              type: event.type,
              detail: event.detail,
            })
          },
        })
      }

      const record = db.launchWorktrees.create({
        key,
        ...worktree,
        releaseAfter: null,
      })
      logger.log("launch_worktree_prepared", {
        launchWorktreeId: record.id,
        worktreeDir: record.worktreeDir,
      })

      return toResponse(record)
    } catch (error) {
      if (worktree) {
        await cleanupSessionWorktree(worktree, { worktreePlugins }).catch((cleanupError) => {
          logger.log("launch_worktree_prepare_cleanup_failed", {
            worktreeDir: worktree?.worktreeDir,
            errorMessage: getErrorMessage(cleanupError),
          })
        })
      }

      throw error
    }
  }

  function release(
    params: ReleaseSessionLaunchWorktreeRequest,
  ): ReleaseSessionLaunchWorktreeResponse {
    const record = findById(params.launchWorktreeId)
    if (!record) {
      return {
        launchWorktreeId: params.launchWorktreeId,
        released: false,
      }
    }

    scheduleRelease(record, "client_release")
    return {
      launchWorktreeId: params.launchWorktreeId,
      released: true,
    }
  }

  function takeCompatible(params: {
    launchWorktreeId: string | undefined
    request: { cwd: string; worktree?: { enabled?: boolean; baseBranchName?: string } }
  }): PreparedSessionWorktree | null {
    if (!params.launchWorktreeId) {
      return null
    }

    const record = findById(params.launchWorktreeId)
    if (!record) {
      return null
    }

    const key = createLaunchWorktreeKey({
      cwd: params.request.cwd,
      baseBranchName: params.request.worktree?.baseBranchName,
    })
    if (params.request.worktree?.enabled !== true || record.key !== key) {
      scheduleRelease(record, "incompatible_launch")
      return null
    }

    cancelReleaseTimer(record.id)
    db.launchWorktrees.delete(record.id)
    return toPreparedSessionWorktree(toWorktreeState(record))
  }

  async function cleanupColdWorktrees() {
    const records = db.launchWorktrees.findMany()
    await Promise.all(records.map((record) => cleanupRecord(record, "daemon_startup")))
  }

  async function close() {
    for (const timer of releaseTimers.values()) {
      clearTimeout(timer)
    }
    releaseTimers.clear()
  }

  return {
    cleanupColdWorktrees,
    close,
    prepare,
    release,
    takeCompatible,
  }
}
