import { z } from "zod"

const formSchemaBrand: unique symbol = Symbol("FormSchema")

/** DOM controls supported by the uncontrolled form runtime. */
export type FormControl = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
/** Flat Zod object schemas accepted by the form descriptor. */
export type AnyObjectSchema = z.ZodObject<z.ZodRawShape>
/** Stable top-level field names declared by one flat form schema. */
export type FieldName<T extends AnyObjectSchema> = keyof z.input<T> & string
/** Keyed validation messages for one flat form schema. */
export type FormErrors<T extends AnyObjectSchema> = {
  readonly [K in FieldName<T>]: string | null
}
/** Keyed callback refs for one flat form schema. */
export type FormRefs<T extends AnyObjectSchema> = {
  readonly [K in FieldName<T>]: (node: FormControl | null) => void
}
/** Structured invalid-submit details for one flat form schema. */
export type FormInvalidResult<T extends AnyObjectSchema> = {
  readonly errors: FormErrors<T>
  readonly error: z.ZodError<z.input<T>>
}
/** Canonical descriptor for one flat Zod-backed DOM form. */
export type FormSchema<T extends AnyObjectSchema> = {
  readonly [formSchemaBrand]: true
  readonly zod: T
  readonly keys: readonly FieldName<T>[]
}

type FlatFieldSchemaNode = {
  def: {
    type: string
    element?: FlatFieldSchemaNode
    in?: FlatFieldSchemaNode
    innerType?: FlatFieldSchemaNode
    left?: FlatFieldSchemaNode
    options?: FlatFieldSchemaNode[]
    right?: FlatFieldSchemaNode
  }
}

/**
 * Creates the canonical descriptor for one flat Zod-backed DOM form and rejects nested field
 * inputs that would require path-based registration.
 */
export function createForm<const TShape extends z.ZodRawShape>(shape: TShape) {
  assertFlatFormShape(shape)

  const zod = z.object(shape)
  const keys = Object.freeze(Object.keys(shape)) as readonly (keyof TShape & string)[]

  return Object.freeze({
    [formSchemaBrand]: true as const,
    zod,
    keys,
  }) as FormSchema<z.ZodObject<TShape>>
}

function assertFlatFormShape(shape: z.ZodRawShape) {
  for (const [fieldName, fieldSchema] of Object.entries(shape)) {
    if (supportsFlatFieldInput(fieldSchema as unknown as FlatFieldSchemaNode)) {
      continue
    }

    throw new Error(
      `createForm only supports flat field inputs. "${fieldName}" requires nested input values.`,
    )
  }
}

const supportsFlatFieldInput: (schema: FlatFieldSchemaNode) => boolean = (schema) => {
  switch (schema.def.type) {
    case "optional":
    case "nullable":
    case "default":
    case "catch":
    case "readonly":
    case "nonoptional":
    case "prefault":
      return schema.def.innerType ? supportsFlatFieldInput(schema.def.innerType) : true

    case "array":
      return schema.def.element ? supportsFlatFieldInput(schema.def.element) : true

    case "pipe":
      return schema.def.in ? supportsFlatFieldInput(schema.def.in) : true

    case "union":
      return (schema.def.options ?? []).every((option) => supportsFlatFieldInput(option))

    case "intersection":
      return schema.def.left && schema.def.right
        ? supportsFlatFieldInput(schema.def.left) && supportsFlatFieldInput(schema.def.right)
        : true

    case "object":
    case "record":
    case "map":
    case "set":
    case "tuple":
    case "lazy":
      return false

    default:
      return true
  }
}
