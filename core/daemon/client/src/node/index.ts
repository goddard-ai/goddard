import { createNodeClient } from "@goddard-ai/ipc/node"
import { GODDARD_DAEMON_SOCKET_FILENAME } from "@goddard-ai/paths"
import { getGoddardGlobalDir } from "@goddard-ai/paths/node"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import * as path from "node:path"
import {
  createDaemonIpcClient as createExplicitDaemonIpcClient,
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
  type DaemonIpcClient,
  type DaemonIpcClientFactory,
  type DaemonIpcClientFactoryInput,
} from "../index.ts"

const ipcPrefix = process.platform === "win32" ? "//./pipe/" : ""

/** Environment variables consumed by daemon client convenience helpers. */
export type DaemonClientEnv = Record<string, string | undefined>

/** Resolved daemon connection settings derived from environment variables. */
export type ResolvedDaemonClientEnv = {
  daemonUrl: string
  socketPath: string
}

export {
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
  type DaemonIpcClient,
  type DaemonIpcClientFactory,
  type DaemonIpcClientFactoryInput,
} from "../index.ts"

/** Returns the default daemon socket path for the local Node host. */
export function getDefaultDaemonSocketPath(): string {
  const socketPath = path.posix.join(
    toPosixPath(getGoddardGlobalDir()),
    GODDARD_DAEMON_SOCKET_FILENAME,
  )
  return ipcPrefix.endsWith("/") && socketPath.startsWith("/")
    ? ipcPrefix + socketPath.slice(1)
    : ipcPrefix + socketPath
}

/** Resolves the daemon URL from explicit environment variables or host defaults. */
export function resolveDaemonUrl(env: DaemonClientEnv = process.env): string {
  if (env.GODDARD_DAEMON_URL) {
    return env.GODDARD_DAEMON_URL
  }

  return createDaemonUrl(env.GODDARD_DAEMON_SOCKET_PATH ?? getDefaultDaemonSocketPath())
}

/** Makes the resolved daemon socket settings explicit for Node hosts. */
export function resolveDaemonConnectionFromEnv(
  env: DaemonClientEnv = process.env,
): ResolvedDaemonClientEnv {
  const daemonUrl = resolveDaemonUrl(env)
  return {
    daemonUrl,
    socketPath: readSocketPathFromDaemonUrl(daemonUrl),
  }
}

/** Creates the default Node daemon IPC client for one explicit daemon URL. */
export function createDaemonIpcClient(options: { daemonUrl: string }): DaemonIpcClient

/** Creates one daemon IPC client for a Node host using either the default or injected transport. */
export function createDaemonIpcClient<TClient>(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory<TClient>
}): TClient

/** Creates one daemon IPC client for a Node host using either the default or injected transport. */
export function createDaemonIpcClient(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory
}): DaemonIpcClient {
  return createExplicitDaemonIpcClient({
    daemonUrl: options.daemonUrl,
    createClient: options.createClient ?? defaultCreateClient,
  })
}

/** Creates one daemon IPC client from Node environment variables or injected env values. */
export function createDaemonIpcClientFromEnv(env?: DaemonClientEnv): {
  daemonUrl: string
  client: DaemonIpcClient
}

/** Creates one daemon IPC client from Node environment variables or injected env values. */
export function createDaemonIpcClientFromEnv<TClient>(options: {
  env?: DaemonClientEnv
  createClient?: DaemonIpcClientFactory<TClient>
}): {
  daemonUrl: string
  client: TClient
}

/** Creates one daemon IPC client from Node environment variables or injected env values. */
export function createDaemonIpcClientFromEnv(
  input:
    | DaemonClientEnv
    | { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory } = process.env,
): {
  daemonUrl: string
  client: DaemonIpcClient
} {
  const env = hasFactoryOptions(input) ? (input.env ?? process.env) : input
  const { daemonUrl } = resolveDaemonConnectionFromEnv(env)

  return {
    daemonUrl,
    client: createDaemonIpcClient({
      daemonUrl,
      createClient: hasFactoryOptions(input) ? input.createClient : undefined,
    }),
  }
}

/** Creates the default Node daemon IPC transport from one socket path. */
function defaultCreateClient(input: { socketPath: string }): DaemonIpcClient {
  return createNodeClient(input.socketPath, daemonIpcSchema)
}

/** Detects the helper overload that carries an env object and injected factory. */
function hasFactoryOptions(
  input: DaemonClientEnv | { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory },
): input is { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory } {
  return "createClient" in input || "env" in input
}

/** Normalizes one host path into a posix-style socket path segment. */
function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
