import { createDaemonUrl, createTcpDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { join } from "node:path"
import { getDefaultDaemonSocketPath } from "./ipc/socket.ts"

/** Environment variables recognized by the daemon runtime. */
export type DaemonRuntimeEnv = Record<string, string | undefined>

/** Explicit daemon launch settings accepted from CLI or tests before env/default resolution. */
export type DaemonRuntimeConfigInput = {
  baseUrl?: string
  socketPath?: string
  tcpHost?: string
  tcpPort?: number
  agentBinDir?: string
  env?: DaemonRuntimeEnv
}

/** Fully resolved daemon runtime contract shared across the daemon entry points. */
export type ResolvedDaemonRuntimeConfig = {
  baseUrl: string
  socketPath: string | null
  tcpHost: string | null
  tcpPort: number | null
  daemonUrl: string
  agentBinDir: string
}

export function resolveDaemonRuntimeConfig(
  input: DaemonRuntimeConfigInput = {},
): ResolvedDaemonRuntimeConfig {
  const env = input.env ?? process.env
  const tcpHost = input.tcpHost ?? env.GODDARD_DAEMON_TCP_HOST ?? null
  const tcpPortSource = input.tcpPort ?? readDaemonTcpPortFromEnv(env)
  const tcpPort = Number.isInteger(tcpPortSource) && (tcpPortSource as number) > 0
    ? (tcpPortSource as number)
    : null
  const socketPath =
    tcpPort === null
      ? (input.socketPath ?? env.GODDARD_DAEMON_SOCKET_PATH ?? getDefaultDaemonSocketPath())
      : null

  return {
    baseUrl: input.baseUrl || env.GODDARD_BASE_URL || "http://127.0.0.1:8787",
    socketPath,
    tcpHost: tcpPort === null ? null : (tcpHost ?? "127.0.0.1"),
    tcpPort,
    daemonUrl:
      tcpPort === null
        ? createDaemonUrl(socketPath)
        : createTcpDaemonUrl(tcpHost ?? "127.0.0.1", tcpPort),
    agentBinDir:
      input.agentBinDir ?? env.GODDARD_AGENT_BIN_DIR ?? join(import.meta.dirname, "../agent-bin"),
  }
}

/** Reads and validates an optional daemon TCP port from environment variables. */
function readDaemonTcpPortFromEnv(env: DaemonRuntimeEnv) {
  const rawValue = env.GODDARD_DAEMON_TCP_PORT
  if (!rawValue) {
    return null
  }

  const parsedPort = Number(rawValue)
  if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65_535) {
    throw new Error("GODDARD_DAEMON_TCP_PORT must be an integer between 1 and 65535")
  }

  return parsedPort
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
