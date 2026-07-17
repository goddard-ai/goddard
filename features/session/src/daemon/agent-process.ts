import { createWriteStream } from "node:fs"
import { mkdir } from "node:fs/promises"
import { join } from "node:path"
import { Readable, Writable } from "node:stream"
import { ReadableStream } from "node:stream/web"
import type { ProcessLike } from "@alloc/tree-kill"
import type { AgentService } from "@goddard-ai/agent/daemon"
import type { ManagedAgentProcessSpec } from "@goddard-ai/agent/daemon/install-service"
import type { DaemonAgentEnvironmentService } from "@goddard-ai/daemon-plugin"
import { getGoddardTempLogDir } from "@goddard-ai/paths/node"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { ManagedAgentsConfig } from "@goddard-ai/schema/config"
import type { AcpAdapterId, AgentInputStream, AgentOutputStream } from "acp-client"
import { getErrorMessage } from "radashi"

import type { SessionEnvPolicyConfig } from "../schema.ts"

/** Describes the concrete child-process invocation for a resolved agent distribution. */
export type AgentProcessSpec = ManagedAgentProcessSpec

/** Callback fired when one Bun-managed agent process exits. */
type AgentProcessExitHandler = (code: number | null, signal: NodeJS.Signals | null) => void

/** Bun subprocess wrapper that preserves the exit hooks and stdio surface the daemon expects. */
export type AgentProcessHandle = ProcessLike & {
  stdin: AgentInputStream
  stdout: AgentOutputStream
  onceExit: (handler: AgentProcessExitHandler) => void
}

/** Builds the child-process environment expected by session agents. */
export function buildAgentProcessEnv(input: {
  daemonUrl: string
  token: string
  createAgentEnvironment: DaemonAgentEnvironmentService["createAgentEnvironment"]
  agentEnv?: Record<string, string>
  sessionEnv?: Record<string, string>
  envPolicy?: SessionEnvPolicyConfig
  hostEnv?: NodeJS.ProcessEnv
}): NodeJS.ProcessEnv {
  const filteredEnv = applySessionEnvPolicy({
    env: {
      ...(input.envPolicy?.inherit === false ? {} : readStringEnv(input.hostEnv ?? process.env)),
      ...input.envPolicy?.set,
      ...input.agentEnv,
      ...input.sessionEnv,
    },
    envPolicy: input.envPolicy,
  })
  const daemonEnv = applySessionEnvPolicy({
    env: input.createAgentEnvironment({ env: filteredEnv }),
    envPolicy: input.envPolicy,
  })

  return {
    ...daemonEnv,
    GODDARD_DAEMON_URL: input.daemonUrl,
    GODDARD_SESSION_TOKEN: input.token,
  }
}

/** Applies configured environment allow/block rules to one concrete environment map. */
function applySessionEnvPolicy(input: {
  env: Record<string, string>
  envPolicy?: SessionEnvPolicyConfig
}) {
  const allowed = input.envPolicy?.allow ? new Set(input.envPolicy.allow) : null
  const blocked = new Set(input.envPolicy?.block ?? [])
  const env: Record<string, string> = {}

  for (const [key, value] of Object.entries(input.env)) {
    if (allowed && !allowed.has(key)) {
      continue
    }
    if (blocked.has(key)) {
      continue
    }
    env[key] = value
  }

  return env
}

/** Normalizes process env input to the string-only map accepted by child processes. */
function readStringEnv(env: NodeJS.ProcessEnv) {
  return Object.fromEntries(
    Object.entries(env).filter((entry): entry is [string, string] => entry[1] !== undefined),
  )
}

/** Wraps Bun's subprocess API with the minimal process hooks used by session management. */
function createAgentProcessHandle(input: {
  cmd: string
  args: string[]
  cwd: string
  env: NodeJS.ProcessEnv
}): AgentProcessHandle {
  let exitState: { code: number | null; signal: NodeJS.Signals | null } | null = null
  const exitHandlers = new Set<AgentProcessExitHandler>()
  const subprocess = Bun.spawn([input.cmd, ...input.args], {
    cwd: input.cwd,
    env: input.env,
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
    onExit(_subprocess, exitCode, signalCode) {
      exitState = {
        code: exitCode,
        signal: signalCode as NodeJS.Signals | null,
      }

      for (const handler of exitHandlers) {
        handler(exitState.code, exitState.signal)
      }
      exitHandlers.clear()
    },
  })

  if (!subprocess.stdin || !subprocess.stdout) {
    throw new Error(`Agent process ${input.cmd} did not expose piped stdio`)
  }

  captureAgentProcessStderr(subprocess)

  const stdin = new Writable({
    write(chunk, _encoding, callback) {
      Promise.resolve(subprocess.stdin!.write(chunk))
        .then(() => callback())
        .catch((error) => {
          callback(error instanceof Error ? error : new Error(getErrorMessage(error)))
        })
    },
    final(callback) {
      Promise.resolve(subprocess.stdin!.end())
        .then(() => callback())
        .catch((error) => {
          callback(error instanceof Error ? error : new Error(getErrorMessage(error)))
        })
    },
  })
  const stdout = Readable.fromWeb(subprocess.stdout as unknown as ReadableStream)

  return {
    stdin,
    stdout,
    pid: subprocess.pid,
    kill(signal) {
      subprocess.kill(signal as never)
      return true
    },
    onceExit(handler) {
      if (exitState) {
        handler(exitState.code, exitState.signal)
        return
      }

      const wrapped: AgentProcessExitHandler = (code, signal) => {
        exitHandlers.delete(wrapped)
        handler(code, signal)
      }
      exitHandlers.add(wrapped)
    },
  }
}

function captureAgentProcessStderr(subprocess: Bun.Subprocess<"pipe", "pipe", "pipe">) {
  if (!subprocess.stderr) {
    return
  }

  mkdir(getGoddardTempLogDir(), { recursive: true })
    .then(() => {
      const stream = createWriteStream(
        join(getGoddardTempLogDir(), `agent-process-${subprocess.pid}.stderr.log`),
        { flags: "a" },
      )
      const stderr = Readable.fromWeb(subprocess.stderr as unknown as ReadableStream)

      stderr.on("data", (chunk: Buffer | string) => {
        process.stderr.write(chunk)
        stream.write(chunk)
      })
      stderr.once("end", () => {
        stream.end()
      })
      stderr.once("error", () => {
        stream.end()
      })
    })
    .catch(() => {})
}

/** Waits until one tracked agent process reports that it has exited. */
export function waitForAgentProcessExit(process: AgentProcessHandle) {
  return new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) => {
    process.onceExit((code, signal) => {
      resolve({ code, signal })
    })
  })
}

/** Resolves and launches the requested agent distribution for a new daemon session. */
export async function spawnAgentProcess(params: {
  daemonUrl: string
  token: string
  agent: AcpAdapterId | AgentDistribution
  cwd: string
  createAgentEnvironment: DaemonAgentEnvironmentService["createAgentEnvironment"]
  env?: Record<string, string>
  envPolicy?: SessionEnvPolicyConfig
  agentService: AgentService
  registry?: Record<string, AgentDistribution>
  managedAgents?: ManagedAgentsConfig
}): Promise<AgentProcessHandle> {
  const { cmd, args, env } = await params.agentService.resolveLaunchProcessSpec({
    agent: params.agent,
    registry: params.registry,
    managedAgents: params.managedAgents,
  })

  return createAgentProcessHandle({
    cmd,
    args: [...args],
    cwd: params.cwd,
    env: buildAgentProcessEnv({
      daemonUrl: params.daemonUrl,
      token: params.token,
      createAgentEnvironment: params.createAgentEnvironment,
      agentEnv: env,
      sessionEnv: params.env,
      envPolicy: params.envPolicy,
    }),
  })
}
