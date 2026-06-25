import { createServer } from "node:net"
import { expect, test } from "bun:test"

import { createLocalHttpUrl, getUnusedTcpPort } from "./dev-ports.ts"

test("createLocalHttpUrl formats a loopback HTTP URL", () => {
  expect(createLocalHttpUrl(41234)).toBe("http://127.0.0.1:41234")
})

test("getUnusedTcpPort returns a port that can be rebound", async () => {
  const port = await getUnusedTcpPort()
  const server = createServer()

  try {
    await new Promise<void>((resolve, reject) => {
      const onError = (error: Error) => {
        server.off("listening", onListening)
        reject(error)
      }
      const onListening = () => {
        server.off("error", onError)
        resolve()
      }

      server.once("error", onError)
      server.once("listening", onListening)
      server.listen(port, "127.0.0.1")
    })
  } finally {
    await new Promise<void>((resolve, reject) => {
      server.close((error) => {
        if (error) {
          reject(error)
          return
        }

        resolve()
      })
    })
  }
})
