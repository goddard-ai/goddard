import { createClient, type AppSchema } from "@goddard-ai/ipc"
import { createTauriTransport } from "./transport.ts"

export function createTauriClient<S extends AppSchema>(socketPath: string, schema: S) {
  return createClient(schema, createTauriTransport(socketPath))
}
