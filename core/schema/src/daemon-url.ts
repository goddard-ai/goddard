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
