/** Node-specific daemon IPC client helpers built on the shared daemon client types. */
import { readFileSync } from "node:fs"
import { createRouteClient, IpcClientError, ndjson, type IpcClientHook } from "@goddard-ai/ipc"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { readDaemonConfigFromRootConfig } from "@goddard-ai/schema/config"
import { createDaemonUrl, DEFAULT_DAEMON_PORT } from "@goddard-ai/schema/daemon-url"
import { getErrorMessage } from "radashi"

import { daemonIpcRoutes } from "../daemon-ipc.ts"
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
  ipcHook?: IpcClientHook
  createClient?: DaemonIpcClientFactory<TClient>
}): TClient
export function createDaemonIpcClient(options: {
  daemonUrl: string
  ipcHook?: IpcClientHook
  createClient?: DaemonIpcClientFactory
}): DaemonIpcClient {
  return (options.createClient ?? createDefaultClient)({
    daemonUrl: options.daemonUrl,
    ipcHook: options.ipcHook,
  })
}

/** Creates one daemon IPC client from Node environment variables or injected env values. */
export function createDaemonIpcClientFromEnv<TClient = DaemonIpcClient>(options?: {
  env?: DaemonClientEnv
  ipcHook?: IpcClientHook
  createClient?: DaemonIpcClientFactory<TClient>
}): {
  daemonUrl: string
  client: TClient
}
export function createDaemonIpcClientFromEnv(
  options: {
    env?: DaemonClientEnv
    ipcHook?: IpcClientHook
    createClient?: DaemonIpcClientFactory
  } = {},
): {
  daemonUrl: string
  client: DaemonIpcClient
} {
  const daemonUrl = resolveDaemonUrl(options.env)

  return {
    daemonUrl,
    client: createDaemonIpcClient({
      daemonUrl,
      ipcHook: options.ipcHook,
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
  return createRouteClient({
    baseURL: input.daemonUrl,
    routes: daemonIpcRoutes,
    plugins: [ndjson.clientPlugin],
    clientHook: input.ipcHook,
    onJsonError: async (response) => {
      const body = (await response.json().catch(() => undefined)) as
        | {
            error?: unknown
            message?: unknown
          }
        | undefined
      if (isStructuredIpcError(body?.error)) {
        throw new IpcClientError({
          code: body.error.code,
          details: body.error.details,
        })
      }

      const message =
        typeof body?.error === "string"
          ? body.error
          : typeof body?.message === "string"
            ? body.message
            : `Request failed with status ${response.status}`
      throw new Error(message)
    },
  }) as DaemonIpcClient
}

function isStructuredIpcError(value: unknown): value is {
  code: string
  details?: unknown
} {
  return (
    typeof value === "object" && value !== null && "code" in value && typeof value.code === "string"
  )
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
      `Global config at ${getGlobalConfigPath()} has an invalid daemon config: ${getErrorMessage(error)}`,
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
