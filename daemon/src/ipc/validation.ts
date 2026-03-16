export function requiredString(value: unknown, field: string): string {
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`${field} is required`)
  }

  return value
}

export function optionalString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new Error(`${field} is required`)
  }

  return value
}

export function optionalOptionalString(value: unknown, field: string): string | undefined {
  if (value === undefined) {
    return undefined
  }
  if (typeof value !== "string") {
    throw new Error(`${field} must be a string`)
  }
  return value
}

export function optionalNumber(value: unknown, field: string): number | undefined {
  if (value === undefined) {
    return undefined
  }
  if (typeof value !== "number" || !Number.isInteger(value)) {
    throw new Error(`${field} must be an integer`)
  }
  return value
}
