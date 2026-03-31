import type { AppSchema, InferResponseType, RequestArguments, ValidRequestName } from "./schema.ts"

/** Maps one IPC schema's request names to their corresponding request handlers. */
export type Handlers<S extends AppSchema> = {
  [K in ValidRequestName<S>]: (
    ...args: RequestArguments<S, K>
  ) => Promise<InferResponseType<S, K>> | InferResponseType<S, K>
}
