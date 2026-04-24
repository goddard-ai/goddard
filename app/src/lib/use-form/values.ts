import type { FormControl } from "./schema.ts"
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

export function setDraftFieldValue(
  values: FormValueRecord,
  fieldName: string,
  fieldValue: unknown,
) {
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

export function compareControlsByDomOrder(leftControl: FormControl, rightControl: FormControl) {
  if (leftControl === rightControl) {
    return 0
  }

  const position = leftControl.compareDocumentPosition(rightControl)

  if (position & Node.DOCUMENT_POSITION_FOLLOWING) {
    return -1
  }

  if (position & Node.DOCUMENT_POSITION_PRECEDING) {
    return 1
  }

  return 0
}

export function getObservedEventName(control: FormControl) {
  if (control instanceof HTMLSelectElement) {
    return "change"
  }

  if (control instanceof HTMLInputElement) {
    return control.type === "checkbox" || control.type === "radio" ? "change" : "input"
  }

  return "input"
}

export function isFormControl(value: Element) {
  return (
    value instanceof HTMLInputElement ||
    value instanceof HTMLTextAreaElement ||
    value instanceof HTMLSelectElement
  )
}

/**
 * Serializes one flat field from its registered controls, with DOM-native coercion for the common
 * uncontrolled input types the hook owns.
 */
export function readFieldValue(fieldControls: readonly FormControl[]) {
  const firstControl = fieldControls[0]

  if (!firstControl) {
    return undefined
  }

  if (firstControl instanceof HTMLTextAreaElement) {
    return firstControl.value
  }

  if (firstControl instanceof HTMLSelectElement) {
    return firstControl.multiple
      ? [...firstControl.selectedOptions].map((option) => option.value)
      : firstControl.value
  }

  if (firstControl.type === "checkbox") {
    if (fieldControls.length === 1) {
      return firstControl.checked
    }

    return fieldControls
      .filter((control): control is HTMLInputElement => control instanceof HTMLInputElement)
      .filter((control) => control.checked)
      .map((control) => control.value)
  }

  if (firstControl.type === "radio") {
    return fieldControls
      .filter((control): control is HTMLInputElement => control instanceof HTMLInputElement)
      .find((control) => control.checked)?.value
  }

  if (firstControl.type === "number" || firstControl.type === "range") {
    return firstControl.value === "" ? undefined : Number(firstControl.value)
  }

  return firstControl.value
}

export function getEmptyFieldValue(fieldControls: readonly FormControl[]) {
  const firstControl = fieldControls[0]

  if (!firstControl) {
    return undefined
  }

  if (firstControl instanceof HTMLSelectElement) {
    return firstControl.multiple ? [] : ""
  }

  if (firstControl instanceof HTMLTextAreaElement) {
    return ""
  }

  if (firstControl.type === "checkbox") {
    return fieldControls.length === 1 ? false : []
  }

  if (firstControl.type === "radio") {
    return undefined
  }

  if (firstControl.type === "number" || firstControl.type === "range") {
    return undefined
  }

  return ""
}

/**
 * Applies one stored flat field value back into uncontrolled DOM controls without dispatching
 * synthetic events, so hydration does not recursively trigger persistence callbacks.
 */
export function writeFieldValue(fieldControls: readonly FormControl[], fieldValue: unknown) {
  const firstControl = fieldControls[0]

  if (!firstControl) {
    return
  }

  if (firstControl instanceof HTMLTextAreaElement) {
    firstControl.value = fieldValue == null ? "" : String(fieldValue)
    return
  }

  if (firstControl instanceof HTMLSelectElement) {
    if (firstControl.multiple) {
      const selectedValues = new Set(
        Array.isArray(fieldValue)
          ? fieldValue.map((value) => String(value))
          : fieldValue == null
            ? []
            : [String(fieldValue)],
      )

      for (const option of firstControl.options) {
        option.selected = selectedValues.has(option.value)
      }

      return
    }

    firstControl.value = fieldValue == null ? "" : String(fieldValue)
    return
  }

  if (firstControl.type === "checkbox") {
    if (fieldControls.length > 1 || Array.isArray(fieldValue)) {
      const selectedValues = new Set(
        Array.isArray(fieldValue) ? fieldValue.map((value) => String(value)) : [],
      )

      for (const control of fieldControls) {
        if (control instanceof HTMLInputElement) {
          control.checked = selectedValues.has(control.value)
        }
      }

      return
    }

    firstControl.checked = Boolean(fieldValue)
    return
  }

  if (firstControl.type === "radio") {
    const selectedValue = fieldValue == null ? null : String(fieldValue)

    for (const control of fieldControls) {
      if (control instanceof HTMLInputElement) {
        control.checked = control.value === selectedValue
      }
    }

    return
  }

  firstControl.value =
    firstControl.type === "number" || firstControl.type === "range"
      ? typeof fieldValue === "number"
        ? String(fieldValue)
        : fieldValue == null
          ? ""
          : String(fieldValue)
      : fieldValue == null
        ? ""
        : String(fieldValue)
}
