import { z } from "zod"

/** Marks a response type at the type level without producing any runtime schema. */
export function $type<T>() {
  return {} as { __unchecked__: T }
}

/** Carries a response type through schema declarations without runtime validation logic. */
type ResponseMarker<T = unknown> = {
  __unchecked__: T
}

/** Declares one server stream payload schema with optional subscription params. */
type StreamDefinition = z.ZodTypeAny | { payload: z.ZodTypeAny; subscription?: z.ZodTypeAny }

/** Declares the request and stream contract for one typed IPC application boundary. */
export type AppSchema = {
  client: {
    requests: Record<
      string,
      {
        payload: z.ZodTypeAny
        response: ResponseMarker
      }
    >
  }
  server: {
    streams: Record<string, StreamDefinition>
  }
}

/** Resolves one stream definition into its payload schema. */
type StreamPayloadSchema<T extends StreamDefinition> = T extends z.ZodTypeAny ? T : T["payload"]

/** Resolves one stream definition into its optional subscription schema. */
type StreamSubscriptionSchema<T extends StreamDefinition> = T extends {
  subscription: infer TSchema extends z.ZodTypeAny
}
  ? TSchema
  : z.ZodVoid

/** Extracts the valid client request names from one IPC schema. */
export type ReqName<S extends AppSchema> = keyof S["client"]["requests"] & string

/** Infers the payload type for one client request in the IPC schema. */
export type ReqPayload<S extends AppSchema, K extends ReqName<S>> = z.infer<
  S["client"]["requests"][K]["payload"]
>

/** Extracts the declared response type for one client request in the IPC schema. */
export type ResType<
  S extends AppSchema,
  K extends ReqName<S>,
> = S["client"]["requests"][K]["response"]["__unchecked__"]

/** Extracts the valid server stream names from one IPC schema. */
export type StrName<S extends AppSchema> = keyof S["server"]["streams"] & string

/** Infers the payload type for one server stream in the IPC schema. */
export type StrPayload<S extends AppSchema, K extends StrName<S>> = z.infer<
  StreamPayloadSchema<S["server"]["streams"][K]>
>

/** Infers the optional subscription params for one server stream in the IPC schema. */
export type StrSubscription<S extends AppSchema, K extends StrName<S>> = z.infer<
  StreamSubscriptionSchema<S["server"]["streams"][K]>
>
