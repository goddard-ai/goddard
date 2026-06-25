import { createServer } from "node:net"

const localDevHost = "127.0.0.1"

export function createLocalHttpUrl(port: number) {
  return `http://${localDevHost}:${port}`
}

export async function getUnusedTcpPort() {
  const server = createServer()

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
    server.listen(0, localDevHost)
  })

  const address = server.address()
  if (!address || typeof address === "string") {
    throw new Error("TCP port probe did not bind to a TCP port")
  }

  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error)
        return
      }

      resolve()
    })
  })

  return address.port
}
