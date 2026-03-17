import { createBackendClient, type BackendClient } from "@goddard-ai/backend/client"
import type { RepoEvent } from "@goddard-ai/schema/backend"
import { FileTokenStorage } from "@goddard-ai/storage"
import { resolveDaemonRuntimeConfig } from "./config.ts"
import { buildPrompt, isFeedbackEvent } from "./feedback.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
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
  runOneShot?: (input: OneShotInput) => Promise<string | null> | string | null
  waitForShutdown?: (close: () => void | Promise<void>) => Promise<void>
  io?: DaemonIo
}

const defaultIo: DaemonIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`),
}

export async function runDaemon(input: RunDaemonInput, deps: RunDaemonDeps = {}): Promise<number> {
  const io = deps.io ?? defaultIo
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
    const client = await createBackendClientImpl(runtime.baseUrl)
    const { owner, repo } = splitRepo(input.repo)
    ipcServer = await startIpcServer(client, {
      socketPath: runtime.socketPath,
      agentBinDir: runtime.agentBinDir,
    })
    const activeIpcServer = ipcServer
    const runningPrs = new Map<string, { cancel: () => void }>()
    const activePrSessions = new Map<string, Set<string>>()
    const subscription = await client.stream.subscribeToRepo({ owner, repo })

    io.stdout(`Daemon subscribed to ${owner}/${repo}. Waiting for PR feedback events...`)

    subscription.on("event", async (payload) => {
      const event = payload as RepoEvent

      if (event.type === "pr.closed") {
        const key = `${event.owner}/${event.repo}#${event.prNumber}`
        const startingSession = runningPrs.get(key)
        if (startingSession) {
          startingSession.cancel()
          runningPrs.delete(key)
        }

        const activeSessions = activePrSessions.get(key)
        if (activeSessions) {
          io.stdout(`PR #${event.prNumber} was closed. Terminating active sessions.`)
          for (const sessionId of activeSessions) {
            void activeIpcServer.sessionManager.shutdownSession(sessionId).catch(() => {})
          }
          activePrSessions.delete(key)
        }
        return
      }

      if (!isFeedbackEvent(event)) {
        return
      }

      const prompt = buildPrompt(event)
      const key = `${event.owner}/${event.repo}#${event.prNumber}`

      if (runningPrs.has(key)) {
        io.stdout(
          `Feedback received for PR #${event.prNumber}, but a session is already starting up. Ignoring for now.`,
        )
        return
      }

      let isCancelled = false
      runningPrs.set(key, {
        cancel: () => {
          isCancelled = true
        },
      })

      try {
        const managed = await client.pr.isManaged({
          owner: event.owner,
          repo: event.repo,
          prNumber: event.prNumber,
        })
        if (isCancelled) {
          return
        }

        if (!managed) {
          io.stdout(`Ignoring ${event.type} on unmanaged PR #${event.prNumber}.`)
          return
        }

        io.stdout(`Launching one-shot pi session for ${event.type} on PR #${event.prNumber}...`)

        if (isCancelled || !runningPrs.has(key)) {
          io.stdout(`Launch aborted for PR #${event.prNumber} (was closed during setup).`)
          return
        }

        const sessionId = await runOneShotImpl({
          event,
          prompt,
          projectDir: input.projectDir,
          daemonUrl: activeIpcServer.daemonUrl,
          agentBinDir: runtime.agentBinDir,
        })

        if (sessionId) {
          if (isCancelled || !runningPrs.has(key)) {
            void activeIpcServer.sessionManager.shutdownSession(sessionId).catch(() => {})
          } else {
            const activeSessions = activePrSessions.get(key) ?? new Set<string>()
            activeSessions.add(sessionId)
            activePrSessions.set(key, activeSessions)
          }
        }

        io.stdout(`One-shot pi session finished for PR #${event.prNumber} (session ${sessionId}).`)
      } catch (error) {
        io.stderr(error instanceof Error ? error.message : String(error))
      } finally {
        runningPrs.delete(key)
      }
    })

    await waitForShutdownImpl(() =>
      Promise.all([Promise.resolve(subscription.close()), activeIpcServer.close()]).then(() => {}),
    )
    return 0
  } catch (error) {
    io.stderr(error instanceof Error ? error.message : String(error))
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
