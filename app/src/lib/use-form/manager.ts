import { Sigma } from "preact-sigma"
import { z } from "zod"

import {
  collectSubmissionValues,
  detachFieldControl,
  focusFirstInvalidField,
  getFieldControls,
  hydrateField,
  sweepDisconnectedControls,
} from "./dom.ts"
import type { AnyObjectSchema, FormControl, FormSchema } from "./schema.ts"
import type { FieldErrorRecord, FormValueRecord } from "./types.ts"
import {
  areValueRecordsEqual,
  assignRecordValue,
  cloneValueRecord,
  createEmptyErrors,
  getEmptyFieldValue,
  getObservedEventName,
  readFieldValue,
  setDraftFieldValue,
  writeFieldValue,
} from "./values.ts"

type FormInvalidDetails = {
  errors: FieldErrorRecord
  error: z.ZodError
}

export class FormCallbacksRuntime {
  onValues?: (values: FormValueRecord) => void
  onInvalid?: (details: FormInvalidDetails) => void
  onSubmit: (values: unknown) => void | Promise<void> = () => {}

  sync(nextCallbacks: {
    onValues?(values: FormValueRecord): void
    onInvalid?(details: FormInvalidDetails): void
    onSubmit(values: unknown): void | Promise<void>
  }) {
    this.onValues = nextCallbacks.onValues
    this.onInvalid = nextCallbacks.onInvalid
    this.onSubmit = nextCallbacks.onSubmit
  }
}

export class FormRuntime {
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

/** Reactive state for one uncontrolled Zod-backed form instance. */
type FormManagerState = {
  draftValues: FormValueRecord
  errors: FieldErrorRecord
  isSubmitting: boolean
}

/** Constructor inputs for one form manager instance. */
type FormManagerSetup = Omit<FormManagerState, "isSubmitting"> & {
  schema: FormSchema<AnyObjectSchema>
  callbacks: FormCallbacksRuntime
  runtime: FormRuntime
  isSubmitting?: boolean
}

/** Local sigma state for one uncontrolled Zod-backed form instance. */
export class FormManager extends Sigma<FormManagerState> {
  /** Zod-backed field contract used for validation and ordering; it is constructor wiring, not form state. */
  #schema: FormSchema<AnyObjectSchema>
  /** Stable callback holder supplied by the hook so callback churn does not become reactive form state. */
  #callbacks: FormCallbacksRuntime
  /** Browser control and dirty-field bookkeeping for uncontrolled inputs. */
  #runtime: FormRuntime

  constructor(setup: FormManagerSetup) {
    super({
      draftValues: setup.draftValues,
      errors: setup.errors,
      isSubmitting: setup.isSubmitting ?? false,
    })

    this.#schema = setup.schema
    this.#callbacks = setup.callbacks
    this.#runtime = setup.runtime
  }

  /** Reconciles new persisted initial values into still-pristine uncontrolled fields. */
  syncInitialValues(nextInitialValues?: Record<string, unknown>) {
    if (this.#runtime.previousInitialValues === nextInitialValues) {
      return
    }

    this.#runtime.previousInitialValues = nextInitialValues
    const nextInitialValueRecord = cloneValueRecord(nextInitialValues)

    for (const fieldName of this.#schema.keys) {
      if (this.#runtime.dirtyFields.has(fieldName)) {
        continue
      }

      this.draftValues = setDraftFieldValue(
        this.draftValues,
        fieldName,
        nextInitialValueRecord[fieldName],
      )
      hydrateField(this.#runtime, this.draftValues, fieldName)
    }
  }

  /** Tracks one connected field control and reapplies the current draft value into it. */
  attachFieldControl(fieldName: string, control: FormControl) {
    const previousFieldName = this.#runtime.fieldNameByControl.get(control)

    if (previousFieldName && previousFieldName !== fieldName) {
      detachFieldControl(this.#runtime, previousFieldName, control)
    }

    control.name = fieldName

    const fieldControls = this.#runtime.controlsByField.get(fieldName) ?? new Set<FormControl>()
    fieldControls.add(control)
    this.#runtime.controlsByField.set(fieldName, fieldControls)
    this.#runtime.fieldNameByControl.set(control, fieldName)
    this.#observeControl(control)

    hydrateField(this.#runtime, this.draftValues, fieldName)
  }

  /** Removes one field control and any listener bookkeeping owned by this form. */
  detachFieldControl(fieldName: string, control: FormControl) {
    detachFieldControl(this.#runtime, fieldName, control)
  }

  /** Prunes controls that have disconnected since the last render pass or submit. */
  sweepDisconnectedControls(fieldName: string) {
    sweepDisconnectedControls(this.#runtime, fieldName)
  }

  /** Mirrors one uncontrolled field change into the draft snapshot and persistence callback. */
  handleFieldChange(fieldName: string) {
    this.#runtime.dirtyFields.add(fieldName)
    this.errors = clearFieldError(this.errors, fieldName)

    const nextValues = setDraftFieldValue(
      this.draftValues,
      fieldName,
      readFieldValue(getFieldControls(this.#runtime, fieldName)),
    )

    this.draftValues = nextValues
    this.commit()
    emitValues(this.#callbacks.onValues, this.#runtime, nextValues)
  }

  /** Clears every registered field back to its empty DOM state and emits the raw values. */
  clear() {
    const nextValues: FormValueRecord = {}

    for (const fieldName of this.#schema.keys) {
      this.#runtime.dirtyFields.add(fieldName)

      const fieldControls = getFieldControls(this.#runtime, fieldName)
      writeFieldValue(fieldControls, getEmptyFieldValue(fieldControls))
      assignRecordValue(nextValues, fieldName, readFieldValue(fieldControls))
    }

    this.draftValues = nextValues
    this.errors = clearAllErrors(this.errors, this.#schema.keys)
    this.commit()
    emitValues(this.#callbacks.onValues, this.#runtime, nextValues)
  }

  /** Restores the latest persisted initial values and marks every field pristine again. */
  reset() {
    const nextValues = cloneValueRecord(this.#runtime.previousInitialValues)

    this.#runtime.dirtyFields.clear()

    for (const fieldName of this.#schema.keys) {
      hydrateField(this.#runtime, nextValues, fieldName)
    }

    this.draftValues = nextValues
    this.errors = clearAllErrors(this.errors, this.#schema.keys)
    this.commit()
    emitValues(this.#callbacks.onValues, this.#runtime, nextValues)
  }

  /** Collects live DOM values, validates them, and runs the async submit callback. */
  async submit(formElement: HTMLFormElement) {
    if (this.isSubmitting) {
      return
    }

    const parsed = this.#schema.zod.safeParse(
      collectSubmissionValues(this.#schema.keys, this.#runtime, formElement),
    )

    if (!parsed.success) {
      const errors = buildFieldErrors(this.#schema.keys, parsed.error)

      this.errors = errors
      this.commit()
      this.#callbacks.onInvalid?.({
        errors,
        error: parsed.error,
      })
      focusFirstInvalidField(this.#runtime, parsed.error, formElement)
      return
    }

    this.errors = clearAllErrors(this.errors, this.#schema.keys)
    this.isSubmitting = true
    this.commit()

    try {
      await this.#callbacks.onSubmit(parsed.data)
    } finally {
      this.isSubmitting = false
      this.commit()
    }
  }

  #observeControl(control: FormControl) {
    if (this.#runtime.cleanupByControl.has(control)) {
      return
    }

    const eventName = getObservedEventName(control)
    const handleControlChange = () => {
      // Controls can be reattached under another field name without reinstalling listeners.
      const fieldName = this.#runtime.fieldNameByControl.get(control)

      if (fieldName) {
        this.handleFieldChange(fieldName)
      }
    }

    control.addEventListener(eventName, handleControlChange)
    this.#runtime.cleanupByControl.set(control, () => {
      control.removeEventListener(eventName, handleControlChange)
    })
  }
}

export interface FormManager extends FormManagerState {}

function emitValues(
  onValues: ((values: FormValueRecord) => void) | undefined,
  runtime: FormRuntime,
  nextValues: FormValueRecord,
) {
  if (!onValues) {
    return
  }

  if (areValueRecordsEqual(runtime.lastEmittedValues, nextValues)) {
    return
  }

  const snapshot = cloneValueRecord(nextValues)
  runtime.lastEmittedValues = snapshot
  onValues(snapshot)
}

function clearFieldError(errors: FieldErrorRecord, fieldName: string) {
  if (errors[fieldName] === null) {
    return errors
  }

  return {
    ...errors,
    [fieldName]: null,
  }
}

function clearAllErrors(errors: FieldErrorRecord, fieldNames: readonly string[]) {
  if (Object.values(errors).every((message) => message === null)) {
    return errors
  }

  return createEmptyErrors(fieldNames)
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
