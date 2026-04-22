/** Node-specific daemon IPC client helpers built on the shared daemon client types. */
import { readFileSync } from "node:fs"
import { createNodeClient } from "@goddard-ai/ipc/node"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { readDaemonConfigFromRootConfig } from "@goddard-ai/schema/config"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import {
  createDaemonUrl,
  DEFAULT_DAEMON_PORT,
  readDaemonTcpAddressFromDaemonUrl,
} from "@goddard-ai/schema/daemon-url"

import {
  type DaemonIpcClient,
  type DaemonIpcClientFactory,
  type DaemonIpcClientFactoryInput,
} from "../index.ts"

/** Environment variables consumed by daemon client convenience helpers. */
export type DaemonClientEnv = Record<string, string | undefined>
export type {
  DaemonIpcClient,
  DaemonIpcClientFactory,
  DaemonIpcClientFactoryInput,
} from "../index.ts"

/** Creates one daemon IPC client for a Node host using either the default or injected transport. */
export function createDaemonIpcClient<TClient = DaemonIpcClient>(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory<TClient>
}): TClient
export function createDaemonIpcClient(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory
}): DaemonIpcClient {
  return (options.createClient ?? createDefaultClient)({
    daemonUrl: options.daemonUrl,
  })
}

/** Creates one daemon IPC client from Node environment variables or injected env values. */
export function createDaemonIpcClientFromEnv<TClient = DaemonIpcClient>(options?: {
  env?: DaemonClientEnv
  createClient?: DaemonIpcClientFactory<TClient>
}): {
  daemonUrl: string
  client: TClient
}
export function createDaemonIpcClientFromEnv(
  options: { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory } = {},
): {
  daemonUrl: string
  client: DaemonIpcClient
} {
  const daemonUrl = resolveDaemonUrl(options.env)

  return {
    daemonUrl,
    client: createDaemonIpcClient({
      daemonUrl,
      createClient: options.createClient,
    }),
  }
}

/** Resolves the daemon URL from explicit environment variables or host defaults. */
export function resolveDaemonUrl(env: DaemonClientEnv = process.env) {
  if (env.GODDARD_DAEMON_URL) {
    return env.GODDARD_DAEMON_URL
  }

  return createDaemonUrl(resolveDaemonPort(env))
}

/** Creates the default Node daemon IPC transport from one daemon URL. */
function createDefaultClient(input: DaemonIpcClientFactoryInput): DaemonIpcClient {
  const address = readDaemonTcpAddressFromDaemonUrl(input.daemonUrl)
  return createNodeClient(address, daemonIpcSchema)
}

function resolveDaemonPort(env: DaemonClientEnv) {
  if (env.GODDARD_DAEMON_PORT) {
    return parseConfiguredPort(env.GODDARD_DAEMON_PORT, "GODDARD_DAEMON_PORT")
  }

  return readDaemonPortFromGlobalConfig() ?? DEFAULT_DAEMON_PORT
}

function readDaemonPortFromGlobalConfig() {
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

function parseConfiguredPort(value: string, label: string) {
  const port = Number(value)
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(`${label} must be an integer TCP port between 1 and 65535`)
  }

  return port
}
