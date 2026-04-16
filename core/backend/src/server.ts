import { createClient } from "@libsql/client"

import { TursoBackendControlPlane } from "./db/persistence.ts"
import { InMemoryBackendControlPlane, startBackendServer } from "./index.ts"

const port = Number(process.env.PORT ?? "8787")
const dbUrl = process.env.DATABASE_URL

let controlPlane
if (dbUrl) {
  process.stdout.write(`Using database at ${dbUrl}\n`)
  const client = createClient({ url: dbUrl })
  controlPlane = new TursoBackendControlPlane(client)
} else {
  process.stdout.write("Using in-memory control plane\n")
  controlPlane = new InMemoryBackendControlPlane()
}

const server = await startBackendServer(controlPlane, { port })
process.stdout.write(`goddard backend listening on http://127.0.0.1:${server.port}\n`)

const shutdown = async () => {
  await server.close()
  process.exit(0)
}

process.on("SIGINT", shutdown)
process.on("SIGTERM", shutdown)
