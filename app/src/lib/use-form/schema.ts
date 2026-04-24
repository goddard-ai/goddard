import { z } from "zod"

export type FormControl = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
export type AnyObjectSchema = z.ZodObject<z.ZodRawShape>
export type FieldName<T extends AnyObjectSchema> = keyof z.input<T> & string
export type FormErrors<T extends AnyObjectSchema> = {
  readonly [K in FieldName<T>]: string | null
}
export type FormRefs<T extends AnyObjectSchema> = {
  readonly [K in FieldName<T>]: (node: FormControl | null) => void
}
export type FormSchema<T extends AnyObjectSchema> = {
  readonly zod: T
  readonly keys: readonly FieldName<T>[]
}

/**
 * Creates a flat Zod-backed form descriptor with stable field keys for the hook runtime.
 */
export function createForm<const TShape extends z.ZodRawShape>(shape: TShape) {
  const zod = z.object(shape)

  return {
    zod,
    keys: Object.freeze(Object.keys(shape)) as readonly (keyof TShape & string)[],
  }
}
