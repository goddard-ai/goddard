import type { DaemonIpcClient, DaemonIpcClientFactory } from "@goddard-ai/daemon-client"
import type {
  InferResponseType,
  InferStreamPayload,
  StreamTarget,
  ValidRequestName,
  ValidStreamName,
} from "@goddard-ai/ipc"
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"

/** Daemon IPC schema used by the SDK facade. */
type IpcSchema = typeof daemonIpcSchema

/** Union of daemon request names accepted by the IPC client. */
type IpcRequestName = ValidRequestName<IpcSchema>

/** Union of daemon stream names accepted by the IPC client. */
type IpcStreamName = ValidStreamName<IpcSchema>

/** Minimal daemon client contract accepted by the SDK without generic method signatures. */
export type GoddardClient = {
  send(name: IpcRequestName, payload?: any): Promise<InferResponseType<IpcSchema, IpcRequestName>>
  subscribe(
    target: StreamTarget<IpcSchema, IpcStreamName>,
    onMessage: (payload: InferStreamPayload<IpcSchema, IpcStreamName>) => void,
  ): Promise<() => void>
}

/** Shared explicit connection options for SDK calls that talk to the daemon over IPC. */
export type IpcClientOptions =
  | {
      client: GoddardClient
    }
  | {
      daemonUrl: string
      createClient: DaemonIpcClientFactory<GoddardClient>
    }

/** Resolves the daemon IPC client from explicit browser-safe connection inputs. */
export function resolveIpcClient(options: IpcClientOptions): DaemonIpcClient {
  if ("client" in options) {
    return options.client
  }
  const { createClient, daemonUrl } = options
  return createClient({
    daemonUrl,
  })
}
