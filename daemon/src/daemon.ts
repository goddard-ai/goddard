import { createBackendClient, type BackendClient } from "@goddard-ai/backend/client"
import type { RepoEvent } from "@goddard-ai/schema/backend"
import { FileTokenStorage } from "@goddard-ai/storage"
import { resolveDaemonRuntimeConfig } from "./config.ts"
import { buildPrompt, isFeedbackEvent } from "./feedback.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import { createDaemonLogger, createPayloadPreview } from "./logging.ts"
import { runOneShot, type OneShotInput } from "./one-shot.ts"
import { splitRepo } from "./utils.ts"

export type RunDaemonInput = {
  repo: string
  projectDir: string
  baseUrl: string
  socketPath?: string
  agentBinDir?: string
}

export type DaemonIo = {
  stdout: (line: string) => void
  stderr: (line: string) => void
}

export type RunDaemonDeps = {
  createBackendClient?: (baseUrl: string) => Promise<BackendClient> | BackendClient
  startIpcServer?: (
    client: BackendClient,
    options: { socketPath: string; agentBinDir: string },
  ) => Promise<DaemonServer>
  runOneShot?: (input: OneShotInput) => Promise<number> | number
  waitForShutdown?: (close: () => void | Promise<void>) => Promise<void>
  io?: DaemonIo
}

const defaultIo: DaemonIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`),
}

export async function runDaemon(input: RunDaemonInput, deps: RunDaemonDeps = {}): Promise<number> {
  const io = deps.io ?? defaultIo
  const logger = createDaemonLogger(io.stdout)
  const runtime = resolveDaemonRuntimeConfig({
    baseUrl: input.baseUrl,
    socketPath: input.socketPath,
    agentBinDir: input.agentBinDir,
  })
  const createBackendClientImpl = deps.createBackendClient ?? defaultCreateBackendClient
  const startIpcServer =
    deps.startIpcServer ??
    ((client, options) =>
      startDaemonServer(client, {
        socketPath: options.socketPath,
        agentBinDir: options.agentBinDir,
      }))
  const runOneShotImpl = deps.runOneShot ?? runOneShot
  const waitForShutdownImpl = deps.waitForShutdown ?? waitForShutdown
  let ipcServer: DaemonServer | undefined

  try {
    logger.log("daemon.startup", {
      repository: input.repo,
      projectDir: input.projectDir,
      baseUrl: runtime.baseUrl,
      socketPath: runtime.socketPath,
      agentBinDir: runtime.agentBinDir,
    })

    const client = await createBackendClientImpl(runtime.baseUrl)
    const { owner, repo } = splitRepo(input.repo)
    ipcServer = await startIpcServer(client, {
      socketPath: runtime.socketPath,
      agentBinDir: runtime.agentBinDir,
    })
    const activeIpcServer = ipcServer
    const runningPrs = new Set<number>()
    const subscription = await client.stream.subscribeToRepo({ owner, repo })

    logger.log("repo.subscription_started", {
      repository: `${owner}/${repo}`,
      daemonUrl: activeIpcServer.daemonUrl,
      socketPath: activeIpcServer.socketPath,
    })

    subscription.on("event", async (payload) => {
      const event = payload as RepoEvent
      if (!isFeedbackEvent(event)) {
        return
      }

      const prompt = buildPrompt(event)

      if (runningPrs.has(event.prNumber)) {
        logger.log("repo.feedback_coalesced", {
          repository: `${event.owner}/${event.repo}`,
          prNumber: event.prNumber,
          feedbackType: event.type,
        })
        return
      }

      runningPrs.add(event.prNumber)

      try {
        const managed = await client.pr.isManaged({
          owner: event.owner,
          repo: event.repo,
          prNumber: event.prNumber,
        })
        if (!managed) {
          logger.log("repo.feedback_ignored", {
            repository: `${event.owner}/${event.repo}`,
            prNumber: event.prNumber,
            feedbackType: event.type,
            reason: "unmanaged_pr",
          })
          return
        }

        logger.log("one_shot.launch", {
          repository: `${event.owner}/${event.repo}`,
          prNumber: event.prNumber,
          feedbackType: event.type,
          prompt: createPayloadPreview(prompt),
        })
        const exitCode = await runOneShotImpl({
          event,
          prompt,
          projectDir: input.projectDir,
          daemonUrl: activeIpcServer.daemonUrl,
          agentBinDir: runtime.agentBinDir,
        })
        logger.log("one_shot.finish", {
          repository: `${event.owner}/${event.repo}`,
          prNumber: event.prNumber,
          feedbackType: event.type,
          exitCode,
        })
      } catch (error) {
        logger.log("one_shot.failed", {
          repository: `${event.owner}/${event.repo}`,
          prNumber: event.prNumber,
          feedbackType: event.type,
          errorMessage: error instanceof Error ? error.message : String(error),
        })
      } finally {
        runningPrs.delete(event.prNumber)
      }
    })

    await waitForShutdownImpl(() =>
      Promise.all([Promise.resolve(subscription.close()), activeIpcServer.close()]).then(() => {}),
    )
    logger.log("daemon.shutdown", {
      repository: `${owner}/${repo}`,
      socketPath: runtime.socketPath,
    })
    return 0
  } catch (error) {
    logger.log("daemon.run_failed", {
      repository: input.repo,
      errorMessage: error instanceof Error ? error.message : String(error),
    })
    return 1
  } finally {
    if (ipcServer) {
      await ipcServer.close().catch(() => {})
    }
  }
}

export async function waitForShutdown(close: () => void | Promise<void>): Promise<void> {
  await new Promise<void>((resolve) => {
    process.once("SIGINT", () => {
      void close()
      resolve()
    })
  })
}

async function defaultCreateBackendClient(baseUrl: string): Promise<BackendClient> {
  return createBackendClient({ baseUrl, tokenStorage: new FileTokenStorage() })
}
