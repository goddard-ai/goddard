import { z } from "zod"

export function $type<T>() {
  return {} as { __unchecked__: T }
}

type ResponseMarker<T = unknown> = {
  __unchecked__: T
}

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
    streams: Record<string, z.ZodTypeAny>
  }
}

export type ReqName<S extends AppSchema> = keyof S["client"]["requests"] & string

export type ReqPayload<S extends AppSchema, K extends ReqName<S>> = z.infer<
  S["client"]["requests"][K]["payload"]
>

export type ResType<
  S extends AppSchema,
  K extends ReqName<S>,
> = S["client"]["requests"][K]["response"]["__unchecked__"]

export type StrName<S extends AppSchema> = keyof S["server"]["streams"] & string

export type StrPayload<S extends AppSchema, K extends StrName<S>> = z.infer<
  S["server"]["streams"][K]
>
