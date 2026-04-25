import { z } from "zod"

import type { FormControl } from "./schema.ts"
import type { FormRefRecord, FormValueRecord } from "./types.ts"
import { assignRecordValue } from "./values.ts"

type FormRefController = {
  attachFieldControl(fieldName: string, control: FormControl): void
  sweepDisconnectedControls(fieldName: string): void
}

/** Creates stable callback refs that attach controls and prune disconnected controls. */
export function createFieldRefs(form: FormRefController, fieldNames: readonly string[]) {
  const nextRefs: FormRefRecord = {}

  for (const fieldName of fieldNames) {
    nextRefs[fieldName] = (node) => {
      if (node) {
        form.attachFieldControl(fieldName, node)
        return
      }

      form.sweepDisconnectedControls(fieldName)
    }
  }

  return nextRefs
}

/**
 * Reads the live values for the controls inside the submitted form instead of relying on stored
 * draft state, so hidden or unmounted fields are not silently submitted.
 */
export function collectSubmissionValues(
  fieldNames: readonly string[],
  fieldNameByControl: WeakMap<FormControl, string>,
  formElement: HTMLFormElement,
) {
  const formControls = [...formElement.elements].filter(
    (control): control is FormControl => isFormControl(control) && fieldNameByControl.has(control),
  )
  const submissionValues: FormValueRecord = {}

  for (const fieldName of fieldNames) {
    assignRecordValue(
      submissionValues,
      fieldName,
      readFieldValue(formControls.filter((control) => control.name === fieldName)),
    )
  }

  return submissionValues
}

/** Focuses the first still-mounted field control that appears in a validation error path. */
export function focusFirstInvalidField(
  error: z.ZodError,
  controlsByField: Map<string, Set<FormControl>>,
  fieldNameByControl: WeakMap<FormControl, string>,
  cleanupByControl: WeakMap<FormControl, () => void>,
  formElement: HTMLFormElement,
) {
  for (const issue of error.issues) {
    const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

    if (!fieldName) {
      continue
    }

    const control = getFieldControls(
      controlsByField,
      fieldNameByControl,
      cleanupByControl,
      fieldName,
      formElement,
    )[0]

    if (control) {
      control.focus()
      return
    }
  }
}

/** Writes one draft field value into all currently connected controls for that field. */
export function hydrateField(
  draftValues: FormValueRecord,
  controlsByField: Map<string, Set<FormControl>>,
  fieldNameByControl: WeakMap<FormControl, string>,
  cleanupByControl: WeakMap<FormControl, () => void>,
  fieldName: string,
) {
  writeFieldValue(
    getFieldControls(controlsByField, fieldNameByControl, cleanupByControl, fieldName),
    draftValues[fieldName],
  )
}

/** Returns connected controls for one field in DOM order, optionally scoped to one form element. */
export function getFieldControls(
  controlsByField: Map<string, Set<FormControl>>,
  fieldNameByControl: WeakMap<FormControl, string>,
  cleanupByControl: WeakMap<FormControl, () => void>,
  fieldName: string,
  formElement?: HTMLFormElement,
) {
  sweepDisconnectedControls(controlsByField, fieldNameByControl, cleanupByControl, fieldName)

  const fieldControls = [...(controlsByField.get(fieldName) ?? [])].filter(
    (control) => !formElement || formElement.contains(control),
  )

  fieldControls.sort(compareControlsByDomOrder)
  return fieldControls
}

/** Installs the control's value-change listener exactly once for the owning form manager. */
export function observeControl(
  fieldNameByControl: WeakMap<FormControl, string>,
  cleanupByControl: WeakMap<FormControl, () => void>,
  control: FormControl,
  onFieldChange: (fieldName: string) => void,
) {
  if (cleanupByControl.has(control)) {
    return
  }

  const eventName = getObservedEventName(control)
  const handleControlChange = () => {
    // Controls can be reattached under another field name without reinstalling listeners.
    const fieldName = fieldNameByControl.get(control)

    if (fieldName) {
      onFieldChange(fieldName)
    }
  }

  control.addEventListener(eventName, handleControlChange)
  cleanupByControl.set(control, () => {
    control.removeEventListener(eventName, handleControlChange)
  })
}

/** Detaches controls that have left the document since their last ref callback. */
export function sweepDisconnectedControls(
  controlsByField: Map<string, Set<FormControl>>,
  fieldNameByControl: WeakMap<FormControl, string>,
  cleanupByControl: WeakMap<FormControl, () => void>,
  fieldName: string,
) {
  for (const control of controlsByField.get(fieldName) ?? []) {
    if (control.isConnected) {
      continue
    }

    detachFieldControl(controlsByField, fieldNameByControl, cleanupByControl, fieldName, control)
  }
}

/** Removes one control from all form-owned DOM bookkeeping and listener cleanup. */
export function detachFieldControl(
  controlsByField: Map<string, Set<FormControl>>,
  fieldNameByControl: WeakMap<FormControl, string>,
  cleanupByControl: WeakMap<FormControl, () => void>,
  fieldName: string,
  control: FormControl,
) {
  controlsByField.get(fieldName)?.delete(control)
  cleanupByControl.get(control)?.()
  cleanupByControl.delete(control)
  fieldNameByControl.delete(control)
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

/** Returns the empty DOM value that matches one field's currently registered control type. */
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

function compareControlsByDomOrder(leftControl: FormControl, rightControl: FormControl) {
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

function getObservedEventName(control: FormControl) {
  if (control instanceof HTMLSelectElement) {
    return "change"
  }

  if (control instanceof HTMLInputElement) {
    return control.type === "checkbox" || control.type === "radio" ? "change" : "input"
  }

  return "input"
}

function isFormControl(value: Element) {
  return (
    value instanceof HTMLInputElement ||
    value instanceof HTMLTextAreaElement ||
    value instanceof HTMLSelectElement
  )
}
