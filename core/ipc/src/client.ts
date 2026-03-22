import {
  type AppSchema,
  type ReqName,
  type ReqPayload,
  type ResType,
  type StrName,
  type StrPayload,
} from "./schema.ts"
import { type IpcTransport } from "./transport.ts"

export function createClient<S extends AppSchema>(schema: S, transport: IpcTransport) {
  async function send<K extends ReqName<S>>(
    name: K,
    payload: ReqPayload<S, K>,
  ): Promise<ResType<S, K>> {
    const validPayload = schema.client.requests[name].payload.parse(payload)
    return (await transport.send(name, validPayload)) as ResType<S, K>
  }

  async function subscribe<K extends StrName<S>>(
    name: K,
    onMessage: (payload: StrPayload<S, K>) => void,
  ): Promise<() => void> {
    if (!Object.hasOwn(schema.server.streams, name)) {
      throw new Error(`Invalid stream: ${name}`)
    }

    return await Promise.resolve(
      transport.subscribe(name, (payload) => {
        const validPayload = schema.server.streams[name].parse(payload) as StrPayload<S, K>
        onMessage(validPayload)
      }),
    )
  }

  return { send, subscribe }
}
