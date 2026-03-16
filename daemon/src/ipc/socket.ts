import { mkdir, rm } from "node:fs/promises"
import { request as httpRequest } from "node:http"
import { homedir } from "node:os"
import * as path from "node:path"
import { ipcPath } from "../ipc-path.ts"

export function getDefaultDaemonSocketPath(home = homedir()): string {
  return ipcPath.resolve(path.posix.join(toPosixPath(home), ".goddard", "daemon.sock"))
}

export function createDaemonUrl(socketPath: string): string {
  const url = new URL("http://unix")
  url.searchParams.set("socketPath", socketPath)
  return url.toString()
}

export function readSocketPathFromDaemonUrl(rawUrl: string): string {
  let url: URL
  try {
    url = new URL(rawUrl)
  } catch {
    throw new Error("GODDARD_DAEMON_URL must be a valid URL")
  }

  if (url.protocol !== "http:" || url.hostname !== "unix") {
    throw new Error("GODDARD_DAEMON_URL must use the local daemon URL format")
  }

  const socketPath = url.searchParams.get("socketPath")
  if (!socketPath) {
    throw new Error("GODDARD_DAEMON_URL is missing socketPath")
  }

  return socketPath
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
    if (code === "ENOENT" || code === "ECONNREFUSED") {
      await rm(socketPath, { force: true }).catch(() => {})
      return
    }

    throw error
  }
}

function requestDaemonSocket(socketPath: string, pathname: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const request = httpRequest(
      {
        socketPath,
        path: pathname,
        method: "GET",
      },
      (response) => {
        response.resume()
        resolve()
      },
    )

    request.once("error", (error) => reject(error))
    request.end()
  })
}

function toPosixPath(value: string): string {
  return value.replaceAll(path.sep, path.posix.sep)
}
