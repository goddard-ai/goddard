import { createDaemonIpcClient, type DaemonIpcClient } from "@goddard-ai/daemon-client/node"
import type {
  DaemonRequestName,
  DaemonRequestResponse,
  DaemonSendInput,
} from "~/shared/desktop-rpc"
import { ensureDaemonRuntime } from "./daemon-runtime.ts"

let daemonClient: DaemonIpcClient | undefined

/** Reuses one daemon IPC client for the Bun host process. */
async function getDaemonClient() {
  if (daemonClient) {
    return daemonClient
  }

  const client = createDaemonIpcClient(await ensureDaemonRuntime())
  daemonClient = client
  return client
}

/** Forwards one daemon IPC request through the Bun host's default daemon client. */
export async function daemonSend<Name extends DaemonRequestName>(
  input: DaemonSendInput<Name>,
): Promise<DaemonRequestResponse<Name>> {
  const client = await getDaemonClient()
  const send = client.send as (
    name: Name,
    payload?: DaemonSendInput<Name>["payload"],
  ) => Promise<unknown>
  return (await send(input.name, input.payload)) as DaemonRequestResponse<Name>
}
