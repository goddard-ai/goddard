import { z } from "zod"

/** Marks a compile-time type without producing any runtime schema. */
export function $type<T>() {
  return {} as { __unchecked__: T }
}

/** Carries a type through schema declarations without runtime validation logic. */
type TypeMarker<T = unknown> = {
  __unchecked__: T
}

type RequestSchema = {
  payload?: z.ZodType
  response: TypeMarker
}

type StreamSchemaWithSubscription = {
  payload: TypeMarker
  subscription?: z.ZodType
}

/** Declares one server stream payload schema with optional subscription params. */
type StreamSchema = TypeMarker | StreamSchemaWithSubscription

/** Declares the request and stream contract for one typed IPC application boundary. */
export type IpcSchema = {
  requests: Record<string, RequestSchema>
  streams: Record<string, StreamSchema>
}

/** Resolves one stream definition into its payload marker. */
type StreamPayloadMarker<T extends StreamSchema> = T extends TypeMarker
  ? T
  : T extends StreamSchemaWithSubscription
    ? T["payload"]
    : never

/** Resolves one stream definition into its optional subscription schema. */
type StreamSubscriptionSchema<T extends StreamSchema> = T extends {
  subscription: infer TSchema extends z.ZodType
}
  ? TSchema
  : z.ZodVoid

/** Extracts the valid client request names from one IPC schema. */
export type ValidRequestName<S extends IpcSchema> = keyof S["requests"] & string

/** Expands one request into the argument tuple accepted by typed send and handler functions. */
export type RequestArguments<
  S extends IpcSchema,
  K extends ValidRequestName<S>,
> = S["requests"][K] extends { payload: z.ZodType } ? [payload: InferRequestPayload<S, K>] : []

/** Infers the payload type for one client request in the IPC schema. */
export type InferRequestPayload<
  S extends IpcSchema,
  K extends ValidRequestName<S>,
> = S["requests"][K] extends {
  payload: infer TSchema extends z.ZodType
}
  ? z.infer<TSchema>
  : undefined

/** Extracts the declared response type for one client request in the IPC schema. */
export type InferResponseType<
  S extends IpcSchema,
  K extends ValidRequestName<S>,
> = S["requests"][K]["response"]["__unchecked__"]

/** Extracts the valid server stream names from one IPC schema. */
export type ValidStreamName<S extends IpcSchema> = keyof S["streams"] & string

/** Infers the payload type for one server stream in the IPC schema. */
export type InferStreamPayload<
  S extends IpcSchema,
  K extends ValidStreamName<S>,
> = StreamPayloadMarker<S["streams"][K]>["__unchecked__"]

/** Infers the optional subscription params for one server stream in the IPC schema. */
export type InferStreamSubscription<S extends IpcSchema, K extends ValidStreamName<S>> = z.infer<
  StreamSubscriptionSchema<S["streams"][K]>
>
