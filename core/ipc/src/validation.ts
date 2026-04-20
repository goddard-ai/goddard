import { z } from "zod"

import { IpcClientError } from "./errors.ts"

/** Shared validation helpers for client-visible IPC payload and stream-filter errors. */

/** Detects the common missing-input case where a request expected one object payload. */
function isMissingObjectInputError(error: z.ZodError) {
  if (error.issues.length !== 1) {
    return false
  }

  const issue = error.issues[0]
  return (
    issue?.code === "invalid_type" &&
    issue.expected === "object" &&
    issue.input === undefined &&
    issue.path.length === 0
  )
}

/** Returns true when one unknown value is a plain record. */
function isRecord(value: unknown) {
  return typeof value === "object" && value !== null
}

/** Narrows one unknown value into a record when possible. */
function asRecord(value: unknown) {
  return isRecord(value) ? (value as Record<string, unknown>) : null
}

/** Formats one object key so shape renderings stay readable for both identifiers and quoted keys. */
function formatObjectKey(key: string) {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key) ? key : JSON.stringify(key)
}

/** Renders one input schema into a compact TypeScript-like shape for human-facing errors. */
function describeExpectedShape(schema: z.ZodType) {
  try {
    return describeJsonSchema(
      z.toJSONSchema(schema, {
        io: "input",
        unrepresentable: "any",
      }),
      0,
    )
  } catch {
    return null
  }
}

/** Converts one JSON Schema node into a compact shape string. */
const describeJsonSchema: (schema: unknown, indent: number) => string = (schema, indent) => {
  const schemaRecord = asRecord(schema)
  if (!schemaRecord) {
    return "unknown"
  }

  if (schemaRecord.const !== undefined) {
    return JSON.stringify(schemaRecord.const)
  }

  if (Array.isArray(schemaRecord.enum) && schemaRecord.enum.length > 0) {
    return schemaRecord.enum.map((value: unknown) => JSON.stringify(value)).join(" | ")
  }

  const variants = [
    ...(Array.isArray(schemaRecord.anyOf) ? schemaRecord.anyOf : []),
    ...(Array.isArray(schemaRecord.oneOf) ? schemaRecord.oneOf : []),
  ]
  if (variants.length > 0) {
    return variants.map((variant) => describeJsonSchema(variant, indent)).join(" | ")
  }

  const properties = asRecord(schemaRecord.properties)
  if (schemaRecord.type === "object" || properties) {
    const propertyEntries = properties ? Object.entries(properties) : []
    if (propertyEntries.length === 0) {
      return "{}"
    }

    const padding = " ".repeat(indent)
    const propertyPadding = " ".repeat(indent + 2)
    const requiredKeys = new Set(
      Array.isArray(schemaRecord.required)
        ? schemaRecord.required.flatMap((value) => (typeof value === "string" ? [value] : []))
        : [],
    )

    return [
      "{",
      ...propertyEntries.map(
        ([key, value]) =>
          `${propertyPadding}${formatObjectKey(key)}${requiredKeys.has(key) ? "" : "?"}: ${describeJsonSchema(value, indent + 2)}`,
      ),
      `${padding}}`,
    ].join("\n")
  }

  if (schemaRecord.type === "array") {
    return `Array<${describeJsonSchema(schemaRecord.items, indent)}>`
  }

  if (Array.isArray(schemaRecord.type) && schemaRecord.type.length > 0) {
    return schemaRecord.type
      .map((typeName: unknown) => describeJsonSchema({ type: typeName }, indent))
      .join(" | ")
  }

  switch (schemaRecord.type) {
    case "boolean":
      return "boolean"
    case "integer":
    case "number":
      return "number"
    case "null":
      return "null"
    case "string":
      return "string"
    default:
      return "unknown"
  }
}

/** Formats one Zod validation failure for the IPC client surface. */
function formatValidationErrorMessage(error: z.ZodError, schema?: z.ZodType) {
  if (schema && isMissingObjectInputError(error)) {
    const expectedShape = describeExpectedShape(schema)
    if (expectedShape) {
      return `Expected input shape:\n${expectedShape}`
    }
  }

  return z.prettifyError(error)
}

/** Re-classifies validation and parse failures as client-visible IPC errors. */
export function toValidationClientError(
  error: unknown,
  options: {
    schema?: z.ZodType
    fallbackMessage: string
  },
) {
  if (error instanceof IpcClientError) {
    return error
  }

  if (error instanceof z.ZodError) {
    return new IpcClientError(formatValidationErrorMessage(error, options.schema), {
      cause: error,
    })
  }

  if (error instanceof Error) {
    return new IpcClientError(options.fallbackMessage, { cause: error })
  }

  return new IpcClientError(options.fallbackMessage)
}
