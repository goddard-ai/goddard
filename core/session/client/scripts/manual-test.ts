import { startServer } from "@goddard-ai/session"

import { startClient } from "../src/index"

try {
  const server = await startServer({ command: "/bin/bash", args: [] })
  console.log("Server started with endpoint:")
  console.log(JSON.stringify(server.endpoint, null, 2))

  console.log("Starting session client...")
  const client = startClient({
    endpoint: server.endpoint,
  })

  const shutdown = async () => {
    console.log("\nShutting down...")
    client.close()
    await server.close()
    process.exit(0)
  }

  process.on("SIGINT", shutdown)
  process.on("SIGTERM", shutdown)
} catch (error) {
  console.error("Error running manual test:", error)
  process.exit(1)
}
