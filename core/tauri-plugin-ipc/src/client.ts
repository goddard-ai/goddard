import { createClient } from "@goddard-ai/ipc/client"
import { type AppSchema } from "@goddard-ai/ipc/schema"
import { createTauriTransport } from "./transport.js"

export function createTauriClient<S extends AppSchema>(socketPath: string, schema: S) {
  return createClient(schema, createTauriTransport(socketPath))
}
