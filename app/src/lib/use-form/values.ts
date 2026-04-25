import type { FieldErrorRecord, FormValueRecord } from "./types.ts"

export function createEmptyErrors(fieldNames: readonly string[]) {
  const errors: FieldErrorRecord = {}

  for (const fieldName of fieldNames) {
    errors[fieldName] = null
  }

  return errors
}

/**
 * Form value snapshots are shallow and flat, but arrays must still be copied so persisted callers
 * do not mutate the hook's internal draft state by reference.
 */
export function cloneValueRecord(values?: Record<string, unknown>) {
  const snapshot: FormValueRecord = {}

  for (const [fieldName, fieldValue] of Object.entries(values ?? {})) {
    snapshot[fieldName] = Array.isArray(fieldValue) ? [...fieldValue] : fieldValue
  }

  return snapshot
}

export function setFormFieldValue(values: FormValueRecord, fieldName: string, fieldValue: unknown) {
  const nextValues = cloneValueRecord(values)
  assignRecordValue(nextValues, fieldName, fieldValue)
  return nextValues
}

export function assignRecordValue(values: FormValueRecord, fieldName: string, fieldValue: unknown) {
  if (fieldValue === undefined) {
    delete values[fieldName]
    return
  }

  values[fieldName] = Array.isArray(fieldValue) ? [...fieldValue] : fieldValue
}

export function areValueRecordsEqual(leftValues: FormValueRecord, rightValues: FormValueRecord) {
  const fieldNames = new Set([...Object.keys(leftValues), ...Object.keys(rightValues)])

  for (const fieldName of fieldNames) {
    if (!areFieldValuesEqual(leftValues[fieldName], rightValues[fieldName])) {
      return false
    }
  }

  return true
}

function areFieldValuesEqual(leftValue: unknown, rightValue: unknown) {
  if (!Array.isArray(leftValue) && !Array.isArray(rightValue)) {
    return leftValue === rightValue
  }

  if (!Array.isArray(leftValue) || !Array.isArray(rightValue)) {
    return false
  }

  if (leftValue.length !== rightValue.length) {
    return false
  }

  return leftValue.every((entry, index) => entry === rightValue[index])
}
