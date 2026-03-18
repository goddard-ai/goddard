import { createNodeClient } from "@goddard-ai/ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl, readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { getGoddardGlobalDir } from "@goddard-ai/storage"
import * as path from "node:path"

const ipcPrefix = process.platform === "win32" ? "//./pipe/" : ""

// Environment variables consumed by daemon client convenience helpers.
export type DaemonClientEnv = Record<string, string | undefined>

// Resolved daemon connection settings derived from environment variables.
export type ResolvedDaemonClientEnv = {
  daemonUrl: string
  socketPath: string
}

// Socket metadata passed to environment-specific IPC client factories.
export type DaemonIpcClientFactoryInput = {
  socketPath: string
}

// IPC client type produced by the default Node transport.
export type DaemonIpcClient = ReturnType<typeof createNodeClient<typeof daemonIpcSchema>>

// Injectable factory for hosts that provide a custom IPC transport.
export type DaemonIpcClientFactory<TClient = DaemonIpcClient> = (
  input: DaemonIpcClientFactoryInput,
) => TClient

export { createDaemonUrl, readSocketPathFromDaemonUrl }

export function getDefaultDaemonSocketPath(): string {
  const socketPath = path.posix.join(toPosixPath(getGoddardGlobalDir()), "daemon.sock")
  return ipcPrefix.endsWith("/") && socketPath.startsWith("/")
    ? ipcPrefix + socketPath.slice(1)
    : ipcPrefix + socketPath
}

export function resolveDaemonUrl(env: DaemonClientEnv = process.env): string {
  if (env.GODDARD_DAEMON_URL) {
    return env.GODDARD_DAEMON_URL
  }

  return createDaemonUrl(env.GODDARD_DAEMON_SOCKET_PATH ?? getDefaultDaemonSocketPath())
}

export function resolveDaemonConnectionFromEnv(
  env: DaemonClientEnv = process.env,
): ResolvedDaemonClientEnv {
  const daemonUrl = resolveDaemonUrl(env)
  return {
    daemonUrl,
    socketPath: readSocketPathFromDaemonUrl(daemonUrl),
  }
}

export function createDaemonIpcClient(options: { daemonUrl: string }): DaemonIpcClient

export function createDaemonIpcClient<TClient>(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory<TClient>
}): TClient

export function createDaemonIpcClient(options: {
  daemonUrl: string
  createClient?: DaemonIpcClientFactory
}): DaemonIpcClient {
  const createClient = options.createClient ?? defaultCreateClient

  return createClient({
    socketPath: readSocketPathFromDaemonUrl(options.daemonUrl),
  })
}

export function createDaemonIpcClientFromEnv(env?: DaemonClientEnv): {
  daemonUrl: string
  client: DaemonIpcClient
}

export function createDaemonIpcClientFromEnv<TClient>(options: {
  env?: DaemonClientEnv
  createClient?: DaemonIpcClientFactory<TClient>
}): {
  daemonUrl: string
  client: TClient
}

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
  const client = createDaemonIpcClient({
    daemonUrl,
    createClient: hasFactoryOptions(input) ? input.createClient : undefined,
  })

  return {
    daemonUrl,
    client,
  }
}

function defaultCreateClient(input: { socketPath: string }): DaemonIpcClient {
  return createNodeClient(input.socketPath, daemonIpcSchema)
}

function hasFactoryOptions(
  input: DaemonClientEnv | { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory },
): input is { env?: DaemonClientEnv; createClient?: DaemonIpcClientFactory } {
  return "createClient" in input || "env" in input
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
