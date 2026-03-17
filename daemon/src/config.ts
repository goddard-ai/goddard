import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { getDefaultDaemonSocketPath } from "./ipc/socket.ts"
import { join } from "node:path"

// Environment variables recognized by the daemon runtime.
export type DaemonRuntimeEnv = Record<string, string | undefined>

// Explicit daemon launch settings accepted from CLI or tests before env/default resolution.
export type DaemonRuntimeConfigInput = {
  baseUrl?: string
  socketPath?: string
  agentBinDir?: string
  env?: DaemonRuntimeEnv
}

// Fully resolved daemon runtime contract shared across the daemon entry points.
export type ResolvedDaemonRuntimeConfig = {
  baseUrl: string
  socketPath: string
  daemonUrl: string
  agentBinDir: string
}

export function resolveDaemonRuntimeConfig(
  input: DaemonRuntimeConfigInput = {},
): ResolvedDaemonRuntimeConfig {
  const env = input.env ?? process.env
  const socketPath =
    input.socketPath ?? env.GODDARD_DAEMON_SOCKET_PATH ?? getDefaultDaemonSocketPath()

  return {
    baseUrl: input.baseUrl || env.GODDARD_BASE_URL || "http://127.0.0.1:8787",
    socketPath,
    daemonUrl: createDaemonUrl(socketPath),
    agentBinDir:
      input.agentBinDir ?? env.GODDARD_AGENT_BIN_DIR ?? join(import.meta.dirname, "../agent-bin"),
  }
}

export function prependAgentBinToPath(
  agentBinDir: string,
  env?: Record<string, string>,
): Record<string, string> {
  const existingPath = env?.PATH ?? process.env.PATH ?? ""

  return {
    ...env,
    PATH: existingPath ? `${agentBinDir}:${existingPath}` : agentBinDir,
  }
}
