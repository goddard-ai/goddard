import { Sigma } from "preact-sigma"
import { z } from "zod"

import {
  collectSubmissionValues,
  detachFieldControl,
  focusFirstInvalidField,
  getEmptyFieldValue,
  getFieldControls,
  hydrateField,
  observeControl,
  readFieldValue,
  sweepDisconnectedControls,
  writeFieldValue,
} from "./dom.ts"
import type { AnyObjectSchema, FormControl, FormInvalidResult, FormSchema } from "./schema.ts"
import type { FieldErrorRecord, FormValueRecord } from "./types.ts"
import {
  areValueRecordsEqual,
  assignRecordValue,
  cloneValueRecord,
  createEmptyErrors,
  setDraftFieldValue,
} from "./values.ts"

declare const formManagerSchemaType: unique symbol

/** Reactive state for one uncontrolled Zod-backed form instance. */
type FormManagerState = {
  draftValues: FormValueRecord
  errors: FieldErrorRecord
  isSubmitting: boolean
}

/** Constructor inputs for one form manager instance. */
type FormManagerSetup<T extends AnyObjectSchema> = {
  schema: FormSchema<T>
  initialValues?: Partial<z.input<T>>
  isSubmitting?: boolean
  onValues?(values: Partial<z.input<T>>): void
  onInvalid?(details: FormInvalidResult<T>): void
  onSubmit(values: z.output<T>): void | Promise<void>
}

/** Local sigma state for one uncontrolled Zod-backed form instance. */
export class FormManager<T extends AnyObjectSchema> extends Sigma<FormManagerState> {
  /** Zod-backed field contract used for validation and ordering; it is constructor wiring, not form state. */
  #schema: FormSchema<T>
  /** Latest values callback invoked only when the raw flat value snapshot actually changes. */
  #onValues: ((values: Partial<z.input<T>>) => void) | undefined
  /** Latest invalid-submit callback invoked after validation errors are committed. */
  #onInvalid: ((details: FormInvalidResult<T>) => void) | undefined
  /** Latest submit callback that receives successfully parsed Zod output. */
  #onSubmit: (values: z.output<T>) => void | Promise<void>
  /** Connected controls grouped by flat field name for hydration, reads, and cleanup. */
  #controlsByField = new Map<string, Set<FormControl>>()
  /** Reverse lookup from DOM control to flat field name, used by delegated listeners and submit reads. */
  #fieldNameByControl = new WeakMap<FormControl, string>()
  /** DOM listener cleanup owned by this form for each observed uncontrolled control. */
  #cleanupByControl = new WeakMap<FormControl, () => void>()
  /** Fields changed by the user and therefore protected from later initial-value hydration. */
  #dirtyFields = new Set<string>()
  /** Last raw value snapshot emitted to `onValues`, used to suppress duplicate notifications. */
  #lastEmittedValues: FormValueRecord
  /** Latest initial value object identity observed by the hook for pristine-field reconciliation. */
  #previousInitialValues: Partial<z.input<T>> | undefined

  constructor(setup: FormManagerSetup<T>) {
    const draftValues = cloneValueRecord(setup.initialValues as Record<string, unknown> | undefined)

    super({
      draftValues,
      errors: createEmptyErrors(setup.schema.keys),
      isSubmitting: setup.isSubmitting ?? false,
    })

    this.#schema = setup.schema
    this.#onValues = setup.onValues
    this.#onInvalid = setup.onInvalid
    this.#onSubmit = setup.onSubmit
    this.#lastEmittedValues = cloneValueRecord(setup.initialValues as Record<string, unknown>)
    this.#previousInitialValues = setup.initialValues
  }

  /** Reconciles new persisted initial values into still-pristine uncontrolled fields. */
  syncInitialValues(nextInitialValues?: Partial<z.input<T>>) {
    if (this.#previousInitialValues === nextInitialValues) {
      return
    }

    this.#previousInitialValues = nextInitialValues
    const nextInitialValueRecord = cloneValueRecord(
      nextInitialValues as Record<string, unknown> | undefined,
    )

    for (const fieldName of this.#schema.keys) {
      if (this.#dirtyFields.has(fieldName)) {
        continue
      }

      this.draftValues = setDraftFieldValue(
        this.draftValues,
        fieldName,
        nextInitialValueRecord[fieldName],
      )
      this.#hydrateField(fieldName, this.draftValues)
    }
  }

  /** Tracks one connected field control and reapplies the current draft value into it. */
  attachFieldControl(fieldName: string, control: FormControl) {
    const previousFieldName = this.#fieldNameByControl.get(control)

    if (previousFieldName && previousFieldName !== fieldName) {
      this.#detachFieldControl(previousFieldName, control)
    }

    control.name = fieldName

    const fieldControls = this.#controlsByField.get(fieldName) ?? new Set<FormControl>()
    fieldControls.add(control)
    this.#controlsByField.set(fieldName, fieldControls)
    this.#fieldNameByControl.set(control, fieldName)
    this.#observeControl(control)

    this.#hydrateField(fieldName, this.draftValues)
  }

  /** Removes one field control and any listener bookkeeping owned by this form. */
  detachFieldControl(fieldName: string, control: FormControl) {
    this.#detachFieldControl(fieldName, control)
  }

  /** Prunes controls that have disconnected since the last render pass or submit. */
  sweepDisconnectedControls(fieldName: string) {
    this.#sweepDisconnectedControls(fieldName)
  }

  /** Mirrors one uncontrolled field change into the draft snapshot and persistence callback. */
  handleFieldChange(fieldName: string) {
    this.#dirtyFields.add(fieldName)
    this.errors = clearFieldError(this.errors, fieldName)

    const nextValues = setDraftFieldValue(
      this.draftValues,
      fieldName,
      readFieldValue(this.#getFieldControls(fieldName)),
    )

    this.draftValues = nextValues
    this.commit()
    this.#emitValues(nextValues)
  }

  /** Clears every registered field back to its empty DOM state and emits the raw values. */
  clear() {
    const nextValues: FormValueRecord = {}

    for (const fieldName of this.#schema.keys) {
      this.#dirtyFields.add(fieldName)

      const fieldControls = this.#getFieldControls(fieldName)
      writeFieldValue(fieldControls, getEmptyFieldValue(fieldControls))
      assignRecordValue(nextValues, fieldName, readFieldValue(fieldControls))
    }

    this.draftValues = nextValues
    this.errors = clearAllErrors(this.errors, this.#schema.keys)
    this.commit()
    this.#emitValues(nextValues)
  }

  /** Restores the latest persisted initial values and marks every field pristine again. */
  reset() {
    const nextValues = cloneValueRecord(
      this.#previousInitialValues as Record<string, unknown> | undefined,
    )

    this.#dirtyFields.clear()

    for (const fieldName of this.#schema.keys) {
      this.#hydrateField(fieldName, nextValues)
    }

    this.draftValues = nextValues
    this.errors = clearAllErrors(this.errors, this.#schema.keys)
    this.commit()
    this.#emitValues(nextValues)
  }

  /** Collects live DOM values, validates them, and runs the async submit callback. */
  async submit(formElement: HTMLFormElement) {
    if (this.isSubmitting) {
      return
    }

    const parsed = this.#schema.zod.safeParse(this.#collectSubmissionValues(formElement))

    if (!parsed.success) {
      const errors = buildFieldErrors(this.#schema.keys, parsed.error)

      this.errors = errors
      this.commit()
      this.#onInvalid?.({
        errors: errors as FormInvalidResult<T>["errors"],
        error: parsed.error as FormInvalidResult<T>["error"],
      })
      this.#focusFirstInvalidField(formElement, parsed.error)
      return
    }

    this.errors = clearAllErrors(this.errors, this.#schema.keys)
    this.isSubmitting = true
    this.commit()

    try {
      await this.#onSubmit(parsed.data)
    } finally {
      this.isSubmitting = false
      this.commit()
    }
  }

  #collectSubmissionValues(formElement: HTMLFormElement) {
    return collectSubmissionValues(this.#schema.keys, this.#fieldNameByControl, formElement)
  }

  #focusFirstInvalidField(formElement: HTMLFormElement, error: z.ZodError) {
    focusFirstInvalidField(
      error,
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      formElement,
    )
  }

  #hydrateField(fieldName: string, draftValues: FormValueRecord) {
    hydrateField(
      draftValues,
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      fieldName,
    )
  }

  #getFieldControls(fieldName: string, formElement?: HTMLFormElement) {
    return getFieldControls(
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      fieldName,
      formElement,
    )
  }

  #observeControl(control: FormControl) {
    observeControl(this.#fieldNameByControl, this.#cleanupByControl, control, (fieldName) => {
      this.handleFieldChange(fieldName)
    })
  }

  #sweepDisconnectedControls(fieldName: string) {
    sweepDisconnectedControls(
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      fieldName,
    )
  }

  #detachFieldControl(fieldName: string, control: FormControl) {
    detachFieldControl(
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      fieldName,
      control,
    )
  }

  #emitValues(nextValues: FormValueRecord) {
    if (!this.#onValues) {
      return
    }

    if (areValueRecordsEqual(this.#lastEmittedValues, nextValues)) {
      return
    }

    const snapshot = cloneValueRecord(nextValues)
    this.#lastEmittedValues = snapshot
    this.#onValues(snapshot as Partial<z.input<T>>)
  }
}

export interface FormManager<T extends AnyObjectSchema> extends FormManagerState {
  readonly [formManagerSchemaType]?: T
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
