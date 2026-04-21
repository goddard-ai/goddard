/** Shared helpers for encoding the daemon connection locator used across hosts. */

/** Fixed loopback origin used on Windows when the daemon cannot bind a local pipe listener. */
export const DEFAULT_DAEMON_LOOPBACK_ORIGIN = "http://127.0.0.1:46173"

/** Encodes one daemon IPC target into the shared daemon URL envelope. */
export function createDaemonUrl(socketPath: string): string {
  const url = new URL("http://unix")
  url.searchParams.set("socketPath", socketPath)
  return url.toString()
}

/** Decodes one daemon IPC target from the shared daemon URL envelope. */
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
