import { readFileSync } from "node:fs"
import { delimiter, join } from "node:path"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { readDaemonConfigFromRootConfig } from "@goddard-ai/schema/config"
import { DEFAULT_DAEMON_PORT } from "@goddard-ai/schema/daemon-url"

/** Environment variables recognized by the daemon runtime. */
export type RuntimeEnv = Record<string, string | undefined>

/** Explicit daemon launch settings accepted from CLI or tests before env/default resolution. */
export type RuntimeConfigInput = {
  baseUrl?: string
  port?: number
  agentBinDir?: string
  env?: RuntimeEnv
}

/** Fully resolved daemon runtime contract shared across the daemon entry points. */
export type ResolvedRuntimeConfig = {
  baseUrl: string
  port: number
  agentBinDir: string
}

export function resolveRuntimeConfig(input: RuntimeConfigInput = {}): ResolvedRuntimeConfig {
  const env = input.env ?? process.env
  const port =
    input.port ??
    resolveConfiguredDaemonPort(env) ??
    readGlobalConfigDaemonPort() ??
    DEFAULT_DAEMON_PORT

  assertRuntimePort(port, "Daemon port")

  return {
    baseUrl: input.baseUrl || env.GODDARD_BASE_URL || "http://127.0.0.1:8787",
    port,
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
    PATH: existingPath ? `${agentBinDir}${delimiter}${existingPath}` : agentBinDir,
  }
}

function resolveConfiguredDaemonPort(env: RuntimeEnv) {
  if (!env.GODDARD_DAEMON_PORT) {
    return undefined
  }

  const port = Number(env.GODDARD_DAEMON_PORT)
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("GODDARD_DAEMON_PORT must be an integer TCP port between 1 and 65535")
  }

  return port
}

function readGlobalConfigDaemonPort() {
  let source: string
  try {
    source = readFileSync(getGlobalConfigPath(), "utf8")
  } catch (error) {
    const code = typeof error === "object" && error && "code" in error ? error.code : undefined
    if (code === "ENOENT") {
      return undefined
    }

    throw new Error(`Failed to read Goddard global config at ${getGlobalConfigPath()}`, {
      cause: error,
    })
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(source)
  } catch (error) {
    throw new Error(`Global config at ${getGlobalConfigPath()} must be valid JSON.`, {
      cause: error,
    })
  }

  try {
    return readDaemonConfigFromRootConfig(parsed)?.port
  } catch (error) {
    throw new Error(
      `Global config at ${getGlobalConfigPath()} has an invalid daemon config: ${
        error instanceof Error ? error.message : String(error)
      }`,
      { cause: error },
    )
  }
}

function assertRuntimePort(port: number, label: string) {
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error(`${label} must be an integer TCP port between 0 and 65535`)
  }
}
