import * as acp from "@agentclientprotocol/sdk"
import { SessionParams, SessionServerLog } from "@goddard-ai/schema/session-server"
import { SessionStorage } from "@goddard-ai/storage"
import { ChildProcess, spawn } from "node:child_process"
import { dirname, join } from "node:path"
import { PassThrough, Readable, Writable } from "node:stream"
import { fileURLToPath } from "node:url"
import ReconnectingWebSocket from "reconnecting-websocket"
import WebSocket from "ws"
import { AgentSession } from "./client-session.js"

const __dirname = dirname(fileURLToPath(import.meta.url))

async function poll<T>(check: () => Promise<T | undefined>): Promise<T> {
  while (true) {
    const result = await check()
    if (result !== undefined) {
      return result
    }
  }
}

export async function readSessionServerLog(output: Readable): Promise<SessionServerLog> {
  const timeout = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error("Timed out waiting for session server startup log")), 10_000),
  )

  const sink = new Writable()
  const stream = acp.ndJsonStream(
    Writable.toWeb(sink),
    Readable.toWeb(output) as ReadableStream<Uint8Array>,
  )

  const reader = stream.readable.getReader()

  return Promise.race([
    poll(async () => {
      const { value, done } = await reader.read()
      if (done) {
        throw new Error("Session server exited before reporting startup status")
      }
      const parsed = SessionServerLog.safeParse(value)
      if (parsed.success) {
        return parsed.data
      }
    }),
    timeout,
  ]).finally(() => {
    reader.cancel().catch(() => {})
  })
}

function createSocket(serverAddress: string) {
  return new ReconnectingWebSocket(`${serverAddress.replace("http", "ws")}/acp`, [], {
    WebSocket,
  })
}

async function waitForSocketOpen(socket: ReconnectingWebSocket): Promise<void> {
  if (socket.readyState === ReconnectingWebSocket.OPEN) {
    return
  }

  await new Promise<void>((resolve, reject) => {
    const onOpen = () => {
      socket.removeEventListener("open", onOpen)
      socket.removeEventListener("error", onError)
      resolve()
    }
    const onError = (event: Event | { error?: unknown; message?: string }) => {
      socket.removeEventListener("open", onOpen)
      socket.removeEventListener("error", onError)
      if ("error" in event && event.error instanceof Error) {
        reject(event.error)
        return
      }
      reject(new Error("Failed to connect to session server websocket"))
    }

    socket.addEventListener("open", onOpen)
    socket.addEventListener("error", onError)
  })
}

async function startSessionServer(params: SessionParams) {
  const serverExecutablePath = join(__dirname, "../bin/session-server")
  const serverProcess = spawn(serverExecutablePath, [JSON.stringify(params)], {
    detached: true,
    stdio: ["ignore", "pipe", "inherit"],
    env: {
      ...process.env,
      ...(params.env ?? {}),
    },
  })

  const startupLog = await readSessionServerLog(serverProcess.stdout)

  if (!startupLog.success) {
    throw new Error(`Failed to start session server: ${startupLog.error}`)
  }
  if (!startupLog.serverId) {
    return null
  }

  serverProcess.stdout.destroy()
  serverProcess.unref()

  return {
    serverProcess,
    startupLog,
  }
}

export async function runAgent(
  params: SessionParams & { oneShot: true },
  handler?: acp.Client,
): Promise<null>

export async function runAgent(
  params: SessionParams & { oneShot?: undefined },
  handler?: acp.Client,
): Promise<AgentSession>

export async function runAgent(
  params: SessionParams,
  handler?: acp.Client,
): Promise<AgentSession | null>

export async function runAgent(
  params: SessionParams,
  handler?: acp.Client,
): Promise<AgentSession | null> {
  let activeSessionId = params.sessionId
  let serverAddress = ""
  let serverProcess: ChildProcess | undefined

  if (activeSessionId) {
    const activeSession = await SessionStorage.get(activeSessionId)
    if (activeSession?.serverAddress) {
      serverAddress = activeSession.serverAddress
    }
  }

  if (!serverAddress) {
    const result = await startSessionServer(params)
    if (!result) {
      return null
    }

    serverProcess = result.serverProcess
    serverAddress = result.startupLog.serverAddress
    activeSessionId = result.startupLog.sessionId
  }

  if (!activeSessionId) {
    throw new Error("Session id unavailable")
  }

  const ws = createSocket(serverAddress)

  const agentInput = new Writable({
    write(chunk, encoding, callback) {
      ws.send(chunk.toString())
      callback()
    },
  })

  const agentOutput = new PassThrough()
  ws.addEventListener("message", (event) => {
    agentOutput.write(`${event.data.toString()}\n`)
  })

  const acpClient = new acp.ClientSideConnection(
    () =>
      handler ?? {
        async requestPermission() {
          return { outcome: { outcome: "cancelled" } }
        },
        async sessionUpdate() {
          // no-op by default
        },
      },
    acp.ndJsonStream(
      Writable.toWeb(agentInput),
      Readable.toWeb(agentOutput) as ReadableStream<Uint8Array>,
    ),
  )

  await waitForSocketOpen(ws)

  return new AgentSession(activeSessionId, acpClient, serverAddress, ws, serverProcess)
}
