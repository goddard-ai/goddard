import { createDaemonUrl, readSocketPathFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { getGoddardGlobalDir } from "@goddard-ai/storage"
import { mkdir, rm } from "node:fs/promises"
import * as path from "node:path"
import { ipcPath } from "../ipc-path.js"

export { createDaemonUrl, readSocketPathFromDaemonUrl }

export function getDefaultDaemonSocketPath(): string {
  return ipcPath.resolve(path.posix.join(toPosixPath(getGoddardGlobalDir()), "daemon.sock"))
}

export async function prepareSocketPath(socketPath: string): Promise<void> {
  if (process.platform === "win32") {
    return
  }

  await mkdir(path.dirname(socketPath), { recursive: true })
  await ensureSocketPathAvailable(socketPath)
}

export async function cleanupSocketPath(socketPath: string): Promise<void> {
  if (process.platform === "win32") {
    return
  }

  await rm(socketPath, { force: true }).catch(() => {})
}

async function ensureSocketPathAvailable(socketPath: string): Promise<void> {
  try {
    await requestDaemonSocket(socketPath, "/health")
    throw new Error(`A Goddard daemon is already listening at ${socketPath}`)
  } catch (error) {
    const code = typeof error === "object" && error && "code" in error ? error.code : undefined
    if (code === "FailedToOpenSocket") {
      await rm(socketPath, { force: true }).catch(() => {})
      return
    }

    throw error
  }
}

async function requestDaemonSocket(socketPath: string, pathname: string): Promise<void> {
  const response = await fetch(`http://localhost${pathname}`, {
    method: "GET",
    unix: socketPath,
  })

  await response.arrayBuffer()
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
