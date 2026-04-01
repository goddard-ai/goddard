import type {
  InferRequestPayload,
  InferResponseType,
  IpcSchema,
  RequestArguments,
  ValidRequestName,
} from "./schema.ts"

/** Resolves one server handler signature for a specific request and request-context type. */
export type Handler<S extends IpcSchema, K extends ValidRequestName<S>, TContext> =
  RequestArguments<S, K> extends []
    ? (context: TContext) => Promise<InferResponseType<S, K>> | InferResponseType<S, K>
    : (
        payload: InferRequestPayload<S, K>,
        context: TContext,
      ) => Promise<InferResponseType<S, K>> | InferResponseType<S, K>

/** Maps one IPC schema's request names to their corresponding request handlers. */
export type Handlers<S extends IpcSchema, TContext = undefined> = {
  [K in ValidRequestName<S>]: Handler<S, K, TContext>
}
