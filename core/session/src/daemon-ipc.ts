import { createNodeClient } from "@goddard-ai/ipc"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl, readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { getGoddardGlobalDir } from "@goddard-ai/storage"
import * as path from "node:path"

const ipcPrefix = process.platform === "win32" ? "//./pipe/" : ""

export function getDefaultDaemonSocketPath(): string {
  const socketPath = path.posix.join(toPosixPath(getGoddardGlobalDir()), "daemon.sock")
  return ipcPrefix.endsWith("/") && socketPath.startsWith("/")
    ? ipcPrefix + socketPath.slice(1)
    : ipcPrefix + socketPath
}

export function resolveDaemonUrl(env: Record<string, string | undefined> = process.env): string {
  return env.GODDARD_DAEMON_URL ?? createDaemonUrl(getDefaultDaemonSocketPath())
}

export function createDaemonIpcClient(env: Record<string, string | undefined> = process.env) {
  const daemonUrl = resolveDaemonUrl(env)
  return {
    daemonUrl,
    client: createNodeClient(readSocketPathFromDaemonUrl(daemonUrl), daemonIpcSchema),
  }
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
