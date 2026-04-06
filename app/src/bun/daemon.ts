import { createDaemonIpcClientFromEnv, type DaemonIpcClient } from "@goddard-ai/daemon-client/node"
import type {
  DaemonRequestName,
  DaemonRequestResponse,
  DaemonSendInput,
} from "~/shared/desktop-rpc"

let daemonClient: DaemonIpcClient | undefined

/** Reuses one daemon IPC client for the Bun host process. */
function getDaemonClient(): DaemonIpcClient {
  if (!daemonClient) {
    daemonClient = createDaemonIpcClientFromEnv().client
  }

  return daemonClient
}

/** Forwards one daemon IPC request through the Bun host's default daemon client. */
export async function daemonSend<Name extends DaemonRequestName>(
  input: DaemonSendInput<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const client = getDaemonClient()
  return (await client.send(input.name, input.payload)) as DaemonRequestResponse<Name>
}
