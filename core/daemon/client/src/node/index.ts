/** Node-specific daemon IPC client helpers built on the shared daemon client types. */
import { readFileSync } from "node:fs"
import { createRouteClient, ndjson } from "@goddard-ai/ipc"
import { getGlobalConfigPath } from "@goddard-ai/paths/node"
import { readDaemonConfigFromRootConfig } from "@goddard-ai/schema/config"
import { daemonIpcRoutes } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl, DEFAULT_DAEMON_PORT } from "@goddard-ai/schema/daemon-url"
import { getErrorMessage } from "radashi"

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
  const routeClient = createRouteClient({
    baseURL: input.daemonUrl,
    routes: daemonIpcRoutes,
    plugins: [ndjson.clientPlugin],
    onJsonError: async (response) => {
      const body = (await response.json().catch(() => undefined)) as
        | { error?: unknown; message?: unknown }
        | undefined
      const message =
        typeof body?.error === "string"
          ? body.error
          : typeof body?.message === "string"
            ? body.message
            : `Request failed with status ${response.status}`
      throw new Error(message)
    },
  }) as Record<string, any>
  const client = wrapRouteClient(routeClient)

  return Object.assign(client, {
    send: (name: string, payload?: any) => selectRouteFunction(client, name)(payload),
    subscribe: async (target: any, onMessage: (payload: any) => void) => {
      const abortController = new AbortController()
      const name = typeof target === "string" ? target : target.name
      const filter = typeof target === "string" ? undefined : target.filter
      const stream = (await selectRouteFunction(client, name)(filter, {
        signal: abortController.signal,
      })) as AsyncIterable<unknown>
      const done = (async () => {
        for await (const payload of stream) {
          onMessage(payload)
        }
      })()

      return () => {
        abortController.abort()
        void done.catch(() => {})
      }
    },
  }) as unknown as DaemonIpcClient
}

function wrapRouteClient(client: Record<string, any>): Record<string, any> {
  const wrappedClient: Record<string, any> = {}

  for (const [key, value] of Object.entries(client)) {
    if (typeof value === "function") {
      wrappedClient[key] = (input?: unknown, options?: unknown) =>
        value(normalizeRouteInput(input), options)
      continue
    }

    wrappedClient[key] =
      value && typeof value === "object" && key !== "clientConfig" ? wrapRouteClient(value) : value
  }

  return wrappedClient
}

function normalizeRouteInput(input: unknown) {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return input
  }

  const entries = Object.entries(input)
  if (entries.length === 1 && (entries[0]?.[0] === "body" || entries[0]?.[0] === "query")) {
    return entries[0][1]
  }

  return input
}

function selectRouteFunction(client: Record<string, any>, name: string) {
  let node: unknown = client
  for (const segment of normalizeRouteName(name).split(".")) {
    if (!node || typeof node !== "object" || !(segment in node)) {
      throw new Error(`Unknown daemon IPC route: ${name}`)
    }
    node = (node as Record<string, unknown>)[segment]
  }

  if (typeof node !== "function") {
    throw new Error(`Daemon IPC route is not callable: ${name}`)
  }

  return node
}

function normalizeRouteName(name: string) {
  return name === "session.message" ? "session.messageEvents" : name
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
