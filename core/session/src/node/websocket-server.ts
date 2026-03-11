import { IncomingMessage } from "node:http"
import { Socket } from "node:net"
import { Server } from "srvx"
import WebSocket, { WebSocketServer } from "ws"

export function createWebSocketHandler(handler: {
  onConnection?: (ws: WebSocket) => void
  onMessage: (message: any, ws: WebSocket) => Promise<void>
}) {
  const wss = new WebSocketServer({ noServer: true })

  const connections = new Set<WebSocket>()
  const onConnection = (ws: WebSocket) => {
    connections.add(ws)
    ws.on("close", () => connections.delete(ws))
    ws.on("message", async (data) => {
      let message: any
      try {
        message = JSON.parse(data.toString())
        await handler.onMessage(message, ws)
      } catch (error) {
        console.error("Error handling message:", { error, message })
        return
      }
    })
    handler.onConnection?.(ws)
  }

  return {
    listen(server: Server, pathname: string) {
      const nodeServer = server.node?.server
      if (!nodeServer) {
        throw new Error("The websocket server requires a Node.js environment")
      }
      nodeServer.on("upgrade", (req: IncomingMessage, socket: Socket, head: Buffer) => {
        const url = new URL(req.url ?? "", server.url)
        if (url.pathname === pathname) {
          wss.handleUpgrade(req, socket, head, onConnection)
        } else {
          socket.destroy()
        }
      })
    },
    broadcast(message: unknown, options?: { exclude?: WebSocket }) {
      const data = typeof message === "string" ? message : JSON.stringify(message)
      for (const ws of connections) {
        if (options?.exclude === ws) {
          continue
        }
        ws.send(data)
      }
    },
    close() {
      wss.close()
      for (const ws of connections) {
        ws.close()
      }
    },
  }
}
