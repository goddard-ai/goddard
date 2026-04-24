import { z } from "zod"

import type { FormControl } from "./schema.ts"
import type { FormRefRecord, FormRefState, FormStateAccess, FormValueRecord } from "./types.ts"
import {
  assignRecordValue,
  compareControlsByDomOrder,
  getObservedEventName,
  isFormControl,
  readFieldValue,
  writeFieldValue,
} from "./values.ts"

export function createFieldRefs(form: FormRefState, fieldNames: readonly string[]) {
  const nextRefs: FormRefRecord = {}

  for (const fieldName of fieldNames) {
    nextRefs[fieldName] = (node) => {
      if (node) {
        observeControl(form, fieldName, node)
        form.attachFieldControl(fieldName, node)
        return
      }

      form.sweepDisconnectedControls(fieldName)
    }
  }

  return nextRefs
}

function observeControl(form: FormRefState, fieldName: string, control: FormControl) {
  if (form.runtime.cleanupByControl.has(control)) {
    return
  }

  const eventName = getObservedEventName(control)
  const handleControlChange = () => {
    form.handleFieldChange(fieldName)
  }

  control.addEventListener(eventName, handleControlChange)
  form.runtime.cleanupByControl.set(control, () => {
    control.removeEventListener(eventName, handleControlChange)
  })
}

/**
 * Reads the live values for the controls inside the submitted form instead of relying on stored
 * draft state, so hidden or unmounted fields are not silently submitted.
 */
export function collectSubmissionValues(form: FormStateAccess, formElement: HTMLFormElement) {
  const formControls = [...formElement.elements].filter(
    (control): control is FormControl =>
      isFormControl(control) && form.runtime.fieldNameByControl.has(control),
  )
  const submissionValues: FormValueRecord = {}

  for (const fieldName of form.schema.keys) {
    assignRecordValue(
      submissionValues,
      fieldName,
      readFieldValue(formControls.filter((control) => control.name === fieldName)),
    )
  }

  return submissionValues
}

export function focusFirstInvalidField(
  form: FormStateAccess,
  error: z.ZodError,
  formElement: HTMLFormElement,
) {
  for (const issue of error.issues) {
    const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

    if (!fieldName) {
      continue
    }

    const control = getFieldControls(form, fieldName, formElement)[0]

    if (control) {
      control.focus()
      return
    }
  }
}

export function hydrateField(form: FormStateAccess, fieldName: string) {
  writeFieldValue(getFieldControls(form, fieldName), form.draftValues[fieldName])
}

export function getFieldControls(
  form: FormStateAccess,
  fieldName: string,
  formElement?: HTMLFormElement,
) {
  sweepDisconnectedControls(form, fieldName)

  const fieldControls = [...(form.runtime.controlsByField.get(fieldName) ?? [])].filter(
    (control) => !formElement || formElement.contains(control),
  )

  fieldControls.sort(compareControlsByDomOrder)
  return fieldControls
}

export function sweepDisconnectedControls(form: FormStateAccess, fieldName: string) {
  for (const control of form.runtime.controlsByField.get(fieldName) ?? []) {
    if (control.isConnected) {
      continue
    }

    detachFieldControl(form, fieldName, control)
  }
}

export function detachFieldControl(form: FormStateAccess, fieldName: string, control: FormControl) {
  form.runtime.controlsByField.get(fieldName)?.delete(control)
  form.runtime.cleanupByControl.get(control)?.()
  form.runtime.cleanupByControl.delete(control)
  form.runtime.fieldNameByControl.delete(control)
}
