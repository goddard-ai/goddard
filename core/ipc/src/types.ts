import type {
  InferRequestPayload,
  InferResponseType,
  IpcSchema,
  RequestArguments,
  ValidRequestName,
} from "./schema.ts"

/** Resolves one server handler signature for a specific request name. */
export type Handler<S extends IpcSchema, K extends ValidRequestName<S>> =
  RequestArguments<S, K> extends []
    ? () => Promise<InferResponseType<S, K>> | InferResponseType<S, K>
    : (
        payload: InferRequestPayload<S, K>,
      ) => Promise<InferResponseType<S, K>> | InferResponseType<S, K>

/** Maps one IPC schema's request names to their corresponding request handlers. */
export type Handlers<S extends IpcSchema> = {
  [K in ValidRequestName<S>]: Handler<S, K>
}
