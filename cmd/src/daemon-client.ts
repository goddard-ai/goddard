import { request as httpRequest } from "node:http"

type SubmitPrResult = {
  number: number
  url: string
}

export function hasDaemonSessionContext(): boolean {
  return Boolean(process.env.GODDARD_DAEMON_URL || process.env.GODDARD_SESSION_TOKEN)
}

export async function submitPrViaDaemon(input: {
  cwd: string
  title: string
  body: string
  head?: string
  base?: string
}): Promise<SubmitPrResult> {
  return requestDaemon("/pr/submit", input)
}

export async function replyPrViaDaemon(input: {
  cwd: string
  message: string
  prNumber?: number
}): Promise<{ success: boolean }> {
  return requestDaemon("/pr/reply", input)
}

async function requestDaemon<T>(pathname: string, body: unknown): Promise<T> {
  const daemonUrl = process.env.GODDARD_DAEMON_URL
  if (!daemonUrl) {
    throw new Error("GODDARD_DAEMON_URL is required")
  }

  const sessionToken = process.env.GODDARD_SESSION_TOKEN
  if (!sessionToken) {
    throw new Error("GODDARD_SESSION_TOKEN is required")
  }

  const socketPath = readSocketPathFromDaemonUrl(daemonUrl)
  const payload = JSON.stringify(body)

  return new Promise<T>((resolve, reject) => {
    const request = httpRequest(
      {
        socketPath,
        path: pathname,
        method: "POST",
        headers: {
          authorization: `Bearer ${sessionToken}`,
          "content-type": "application/json",
          "content-length": Buffer.byteLength(payload),
        },
      },
      (response) => {
        let raw = ""
        response.setEncoding("utf8")
        response.on("data", (chunk) => {
          raw += chunk
        })
        response.on("end", () => {
          const data = raw ? JSON.parse(raw) : {}
          if ((response.statusCode ?? 500) >= 400) {
            reject(new Error(data.error ?? `Daemon request failed with status ${response.statusCode}`))
            return
          }

          resolve(data as T)
        })
      },
    )

    request.once("error", (error) => reject(error))
    request.write(payload)
    request.end()
  })
}

function readSocketPathFromDaemonUrl(rawUrl: string): string {
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
