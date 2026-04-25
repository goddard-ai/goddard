import { z } from "zod"

import type { FormRuntime } from "./manager.ts"
import type { FormControl } from "./schema.ts"
import type { FormRefRecord, FormValueRecord } from "./types.ts"
import { assignRecordValue, isFormControl, readFieldValue, writeFieldValue } from "./values.ts"

type FormRefController = {
  attachFieldControl(fieldName: string, control: FormControl): void
  sweepDisconnectedControls(fieldName: string): void
}

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
  runtime: FormRuntime,
  formElement: HTMLFormElement,
) {
  const formControls = [...formElement.elements].filter(
    (control): control is FormControl =>
      isFormControl(control) && runtime.fieldNameByControl.has(control),
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

export function focusFirstInvalidField(
  runtime: FormRuntime,
  error: z.ZodError,
  formElement: HTMLFormElement,
) {
  for (const issue of error.issues) {
    const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

    if (!fieldName) {
      continue
    }

    const control = getFieldControls(runtime, fieldName, formElement)[0]

    if (control) {
      control.focus()
      return
    }
  }
}

export function hydrateField(
  runtime: FormRuntime,
  draftValues: FormValueRecord,
  fieldName: string,
) {
  writeFieldValue(getFieldControls(runtime, fieldName), draftValues[fieldName])
}

export function getFieldControls(
  runtime: FormRuntime,
  fieldName: string,
  formElement?: HTMLFormElement,
) {
  sweepDisconnectedControls(runtime, fieldName)

  const fieldControls = [...(runtime.controlsByField.get(fieldName) ?? [])].filter(
    (control) => !formElement || formElement.contains(control),
  )

  fieldControls.sort(compareControlsByDomOrder)
  return fieldControls
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

export function sweepDisconnectedControls(runtime: FormRuntime, fieldName: string) {
  for (const control of runtime.controlsByField.get(fieldName) ?? []) {
    if (control.isConnected) {
      continue
    }

    detachFieldControl(runtime, fieldName, control)
  }
}

export function detachFieldControl(runtime: FormRuntime, fieldName: string, control: FormControl) {
  runtime.controlsByField.get(fieldName)?.delete(control)
  runtime.cleanupByControl.get(control)?.()
  runtime.cleanupByControl.delete(control)
  runtime.fieldNameByControl.delete(control)
}
