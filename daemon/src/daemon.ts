import { createSdk, type RepoEvent } from "@goddard-ai/sdk"
import { FileTokenStorage } from "@goddard-ai/storage"
import { buildPrompt, isFeedbackEvent } from "./feedback.ts"
import { startDaemonServer, type DaemonServer } from "./ipc.ts"
import { runOneShot, type OneShotInput } from "./one-shot.ts"
import { splitRepo } from "./utils.ts"

export type RunDaemonInput = {
  repo: string
  projectDir: string
  baseUrl: string
}

export type DaemonIo = {
  stdout: (line: string) => void
  stderr: (line: string) => void
}

type StreamSubscription = {
  on: (eventName: string, handler: (payload?: unknown) => void) => StreamSubscription
  close: () => void | Promise<void>
}

type SdkClient = {
  pr: {
    create: (input: {
      owner: string
      repo: string
      title: string
      body?: string
      head: string
      base: string
    }) => Promise<{ number: number; url: string }>
    reply: (input: {
      owner: string
      repo: string
      prNumber: number
      body: string
    }) => Promise<{ success: boolean }>
    isManaged: (input: { owner: string; repo: string; prNumber: number }) => Promise<boolean>
  }
  stream: {
    subscribeToRepo: (repo: { owner: string; repo: string }) => Promise<StreamSubscription>
  }
}

export type RunDaemonDeps = {
  createSdkClient?: (baseUrl: string) => Promise<SdkClient> | SdkClient
  startIpcServer?: (sdk: SdkClient) => Promise<DaemonServer>
  runOneShot?: (input: OneShotInput) => Promise<number> | number
  waitForShutdown?: (close: () => void | Promise<void>) => Promise<void>
  io?: DaemonIo
}

const defaultIo: DaemonIo = {
  stdout: (line) => process.stdout.write(`${line}\n`),
  stderr: (line) => process.stderr.write(`${line}\n`),
}

export async function runDaemon(
  input: RunDaemonInput,
  deps: RunDaemonDeps = {},
): Promise<number> {
  const io = deps.io ?? defaultIo
  const baseUrl = input.baseUrl || process.env.GODDARD_BASE_URL || "http://127.0.0.1:8787"
  const createSdkClient = deps.createSdkClient ?? defaultCreateSdkClient
  const startIpcServer = deps.startIpcServer ?? ((sdk) => startDaemonServer(sdk))
  const runOneShotImpl = deps.runOneShot ?? runOneShot
  const waitForShutdownImpl = deps.waitForShutdown ?? waitForShutdown
  let ipcServer: DaemonServer | undefined

  try {
    const sdk = await createSdkClient(baseUrl)
    const { owner, repo } = splitRepo(input.repo)
    ipcServer = await startIpcServer(sdk)
    const activeIpcServer = ipcServer
    const runningPrs = new Set<number>()
    const subscription = await sdk.stream.subscribeToRepo({ owner, repo })

    io.stdout(`Daemon subscribed to ${owner}/${repo}. Waiting for PR feedback events...`)

    subscription.on("event", async (payload) => {
      const event = payload as RepoEvent
      if (!isFeedbackEvent(event)) {
        return
      }

      const prompt = buildPrompt(event)

      if (runningPrs.has(event.prNumber)) {
        io.stdout(
          `Feedback received for PR #${event.prNumber}, but a session is already running. Ignoring for now.`,
        )
        return
      }

      runningPrs.add(event.prNumber)

      try {
        const managed = await sdk.pr.isManaged({
          owner: event.owner,
          repo: event.repo,
          prNumber: event.prNumber,
        })
        if (!managed) {
          io.stdout(`Ignoring ${event.type} on unmanaged PR #${event.prNumber}.`)
          return
        }

        io.stdout(`Launching one-shot pi session for ${event.type} on PR #${event.prNumber}...`)
        const exitCode = await runOneShotImpl({
          event,
          prompt,
          projectDir: input.projectDir,
          env: {
            GODDARD_DAEMON_URL: activeIpcServer.daemonUrl,
          },
        })
        io.stdout(`One-shot pi session finished for PR #${event.prNumber} (exit ${exitCode}).`)
      } catch (error) {
        io.stderr(error instanceof Error ? error.message : String(error))
      } finally {
        runningPrs.delete(event.prNumber)
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

async function defaultCreateSdkClient(baseUrl: string): Promise<SdkClient> {
  return createSdk({ baseUrl, tokenStorage: new FileTokenStorage() }) as SdkClient
}
