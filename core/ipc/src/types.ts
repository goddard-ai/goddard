import type { AppSchema, ReqName, ReqPayload, ResType } from "./schema.ts"

/** Maps one IPC schema's request names to their corresponding request handlers. */
export type Handlers<S extends AppSchema> = {
  [K in ReqName<S>]: (payload: ReqPayload<S, K>) => Promise<ResType<S, K>> | ResType<S, K>
}
