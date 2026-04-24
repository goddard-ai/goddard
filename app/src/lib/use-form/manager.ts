import { SigmaType } from "preact-sigma"
import { z } from "zod"

import {
  collectSubmissionValues,
  detachFieldControl,
  focusFirstInvalidField,
  getFieldControls,
  hydrateField,
  sweepDisconnectedControls,
} from "./dom.ts"
import type { FormControl } from "./schema.ts"
import type {
  FormCallbacksState,
  FormManagerShape,
  FormRuntimeState,
  FormStateAccess,
  FormValueRecord,
  MutableFormErrorState,
} from "./types.ts"
import {
  assignRecordValue,
  areValueRecordsEqual,
  cloneValueRecord,
  createEmptyErrors,
  getEmptyFieldValue,
  readFieldValue,
  setDraftFieldValue,
  writeFieldValue,
} from "./values.ts"

export class FormCallbacksRuntime implements FormCallbacksState {
  onChange?: (values: FormValueRecord) => void
  onInvalid?: (error: z.ZodError) => void
  onSubmit: (values: unknown) => void | Promise<void> = () => {}

  sync(nextCallbacks: FormCallbacksState) {
    this.onChange = nextCallbacks.onChange
    this.onInvalid = nextCallbacks.onInvalid
    this.onSubmit = nextCallbacks.onSubmit
  }
}

export class FormRuntime implements FormRuntimeState {
  controlsByField = new Map<string, Set<FormControl>>()
  fieldNameByControl = new WeakMap<FormControl, string>()
  cleanupByControl = new WeakMap<FormControl, () => void>()
  dirtyFields = new Set<string>()
  lastEmittedValues: FormValueRecord
  previousInitialValues: Record<string, unknown> | undefined

  constructor(initialValues?: Record<string, unknown>) {
    this.lastEmittedValues = cloneValueRecord(initialValues)
    this.previousInitialValues = initialValues
  }
}

/** Local sigma state for one uncontrolled Zod-backed form instance. */
export const FormManager = new SigmaType<FormManagerShape>("FormManager")
  .defaultState({
    draftValues: {},
    errors: {},
    isSubmitting: false,
  })
  .actions({
    /** Reconciles new persisted initial values into still-pristine uncontrolled fields. */
    syncInitialValues(nextInitialValues?: Record<string, unknown>) {
      if (this.runtime.previousInitialValues === nextInitialValues) {
        return
      }

      this.runtime.previousInitialValues = nextInitialValues
      const nextInitialValueRecord = cloneValueRecord(nextInitialValues)

      for (const fieldName of this.schema.keys) {
        if (this.runtime.dirtyFields.has(fieldName)) {
          continue
        }

        this.draftValues = setDraftFieldValue(
          this.draftValues,
          fieldName,
          nextInitialValueRecord[fieldName],
        )
        hydrateField(this, fieldName)
      }
    },

    /** Tracks one connected field control and reapplies the current draft value into it. */
    attachFieldControl(fieldName: string, control: FormControl) {
      const previousFieldName = this.runtime.fieldNameByControl.get(control)

      if (previousFieldName && previousFieldName !== fieldName) {
        detachFieldControl(this, previousFieldName, control)
      }

      control.name = fieldName

      const fieldControls = this.runtime.controlsByField.get(fieldName) ?? new Set<FormControl>()
      fieldControls.add(control)
      this.runtime.controlsByField.set(fieldName, fieldControls)
      this.runtime.fieldNameByControl.set(control, fieldName)

      hydrateField(this, fieldName)
    },

    /** Removes one field control and any listener bookkeeping owned by this form. */
    detachFieldControl(fieldName: string, control: FormControl) {
      detachFieldControl(this, fieldName, control)
    },

    /** Prunes controls that have disconnected since the last render pass or submit. */
    sweepDisconnectedControls(fieldName: string) {
      sweepDisconnectedControls(this, fieldName)
    },

    /** Mirrors one uncontrolled field change into the draft snapshot and persistence callback. */
    handleFieldChange(fieldName: string) {
      this.runtime.dirtyFields.add(fieldName)
      clearFieldError(this, fieldName)

      const nextValues = setDraftFieldValue(
        this.draftValues,
        fieldName,
        readFieldValue(getFieldControls(this, fieldName)),
      )

      this.draftValues = nextValues
      emitChange(this, nextValues)
    },

    /** Clears every registered field back to its empty DOM state and emits the raw values. */
    clear() {
      const nextValues: FormValueRecord = {}

      for (const fieldName of this.schema.keys) {
        this.runtime.dirtyFields.add(fieldName)

        const fieldControls = getFieldControls(this, fieldName)
        writeFieldValue(fieldControls, getEmptyFieldValue(fieldControls))
        assignRecordValue(nextValues, fieldName, readFieldValue(fieldControls))
      }

      this.draftValues = nextValues
      clearAllErrors(this)
      emitChange(this, nextValues)
    },

    /** Collects live DOM values, validates them, and runs the async submit callback. */
    async submit(formElement: HTMLFormElement) {
      if (this.isSubmitting) {
        return
      }

      const parsed = this.schema.zod.safeParse(collectSubmissionValues(this, formElement))

      if (!parsed.success) {
        this.errors = buildFieldErrors(this.schema.keys, parsed.error)
        this.callbacks.onInvalid?.(parsed.error)
        focusFirstInvalidField(this, parsed.error, formElement)
        this.commit()
        return
      }

      clearAllErrors(this)
      this.isSubmitting = true
      this.commit()

      try {
        await this.callbacks.onSubmit(parsed.data)
      } finally {
        this.isSubmitting = false
        this.commit()
      }
    },
  })

function emitChange(form: FormStateAccess, nextValues: FormValueRecord) {
  const onChange = form.callbacks.onChange

  if (!onChange) {
    return
  }

  if (areValueRecordsEqual(form.runtime.lastEmittedValues, nextValues)) {
    return
  }

  const snapshot = cloneValueRecord(nextValues)
  form.runtime.lastEmittedValues = snapshot
  onChange(snapshot)
}

function clearFieldError(form: MutableFormErrorState, fieldName: string) {
  if (form.errors[fieldName] === null) {
    return
  }

  form.errors = {
    ...form.errors,
    [fieldName]: null,
  }
}

function clearAllErrors(form: MutableFormErrorState) {
  if (Object.values(form.errors).every((message) => message === null)) {
    return
  }

  form.errors = createEmptyErrors(form.schema.keys)
}

function buildFieldErrors(fieldNames: readonly string[], error: z.ZodError) {
  const nextErrors = createEmptyErrors(fieldNames)

  for (const issue of error.issues) {
    const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

    if (!fieldName || nextErrors[fieldName] !== null) {
      continue
    }

    nextErrors[fieldName] = issue.message
  }

  return nextErrors
}
