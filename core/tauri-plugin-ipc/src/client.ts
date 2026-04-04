import { createClient, type IpcSchema } from "@goddard-ai/ipc"
import { createTauriTransport } from "./transport.ts"

export function createTauriClient<S extends IpcSchema>(socketPath: string, schema: S) {
  return createClient(schema, createTauriTransport(socketPath))
}
