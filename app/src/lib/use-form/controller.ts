import { Sigma } from "preact-sigma"
import { z } from "zod"

import {
  collectSubmissionValues,
  createFieldRefs,
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
import type {
  AnyObjectSchema,
  FormControl,
  FormErrors,
  FormInvalidResult,
  FormRefs,
  FormSchema,
} from "./schema.ts"
import type { FieldErrorRecord, FormValueRecord } from "./types.ts"
import {
  areValueRecordsEqual,
  assignRecordValue,
  cloneValueRecord,
  createEmptyErrors,
  setFormFieldValue,
} from "./values.ts"

declare const formControllerSchemaType: unique symbol

/** Reactive state for one uncontrolled Zod-backed form instance. */
type FormControllerState<T extends AnyObjectSchema> = {
  values: FormValueRecord
  errors: FormErrors<T>
  isSubmitting: boolean
}

/** Constructor inputs for one form controller instance. */
type FormControllerSetup<T extends AnyObjectSchema> = {
  schema: FormSchema<T>
  defaultValues?: Partial<z.input<T>>
  isSubmitting?: boolean
  onValuesChange?(values: Partial<z.input<T>>): void
  onInvalid?(details: FormInvalidResult<T>): void
  onSubmit(values: z.output<T>): void | Promise<void>
}

/** Local sigma state for one uncontrolled Zod-backed form instance. */
export class FormController<T extends AnyObjectSchema> extends Sigma<FormControllerState<T>> {
  /** Zod-backed field contract used for validation and ordering; it is constructor wiring, not form state. */
  #schema: FormSchema<T>
  /** Stable field refs exposed to components; each callback delegates back to this controller. */
  #refs: FormRefs<T>
  /** Latest values callback invoked only when the raw flat value snapshot actually changes. */
  #onValuesChange: ((values: Partial<z.input<T>>) => void) | undefined
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
  /** Fields changed by the user and therefore protected from later default-value hydration. */
  #dirtyFields = new Set<string>()
  /** Last raw value snapshot emitted to `onValuesChange`, used to suppress duplicate notifications. */
  #lastEmittedValues: FormValueRecord
  /** Latest default value object identity observed by the hook for pristine-field reconciliation. */
  #defaultValues: Partial<z.input<T>> | undefined

  constructor(setup: FormControllerSetup<T>) {
    const values = cloneValueRecord(setup.defaultValues as Record<string, unknown> | undefined)

    super({
      values,
      errors: createFormErrors<T>(setup.schema.keys),
      isSubmitting: setup.isSubmitting ?? false,
    })

    this.#schema = setup.schema
    this.#refs = createFieldRefs(this, setup.schema.keys) as FormRefs<T>
    this.#onValuesChange = setup.onValuesChange
    this.#onInvalid = setup.onInvalid
    this.#onSubmit = setup.onSubmit
    this.#lastEmittedValues = cloneValueRecord(setup.defaultValues as Record<string, unknown>)
    this.#defaultValues = setup.defaultValues
  }

  get refs() {
    return this.#refs
  }

  handleSubmit = (event: preact.TargetedSubmitEvent<HTMLFormElement>) => {
    event.preventDefault()
    void this.submit(event.currentTarget)
  }

  /** Reconciles new default values into still-pristine uncontrolled fields. */
  syncDefaultValues(nextDefaultValues?: Partial<z.input<T>>) {
    if (this.#defaultValues === nextDefaultValues) {
      return
    }

    this.#defaultValues = nextDefaultValues
    const nextDefaultValueRecord = cloneValueRecord(
      nextDefaultValues as Record<string, unknown> | undefined,
    )

    for (const fieldName of this.#schema.keys) {
      if (this.#dirtyFields.has(fieldName)) {
        continue
      }

      this.values = setFormFieldValue(this.values, fieldName, nextDefaultValueRecord[fieldName])
      this.#hydrateField(fieldName, this.values)
    }
  }

  /** Tracks one connected field control and reapplies the current form value into it. */
  attachFieldControl(fieldName: string, control: FormControl) {
    const previousFieldName = this.#fieldNameByControl.get(control)

    if (previousFieldName && previousFieldName !== fieldName) {
      detachFieldControl(
        this.#controlsByField,
        this.#fieldNameByControl,
        this.#cleanupByControl,
        previousFieldName,
        control,
      )
    }

    control.name = fieldName

    const fieldControls = this.#controlsByField.get(fieldName) ?? new Set<FormControl>()
    fieldControls.add(control)
    this.#controlsByField.set(fieldName, fieldControls)
    this.#fieldNameByControl.set(control, fieldName)
    this.#observeControl(control)

    this.#hydrateField(fieldName, this.values)
  }

  /** Removes one field control and any listener bookkeeping owned by this form. */
  detachFieldControl(fieldName: string, control: FormControl) {
    detachFieldControl(
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      fieldName,
      control,
    )
  }

  /** Prunes controls that have disconnected since the last render pass or submit. */
  sweepDisconnectedControls(fieldName: string) {
    sweepDisconnectedControls(
      this.#controlsByField,
      this.#fieldNameByControl,
      this.#cleanupByControl,
      fieldName,
    )
  }

  /** Mirrors one uncontrolled field change into the current value snapshot and change callback. */
  handleFieldChange(fieldName: string) {
    this.#dirtyFields.add(fieldName)
    this.errors = clearFieldError<T>(this.errors, fieldName)

    const nextValues = setFormFieldValue(
      this.values,
      fieldName,
      readFieldValue(this.#getFieldControls(fieldName)),
    )

    this.values = nextValues
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

    this.values = nextValues
    this.errors = clearAllErrors<T>(this.errors, this.#schema.keys)
    this.commit()
    this.#emitValues(nextValues)
  }

  /** Restores the latest default values and marks every field pristine again. */
  reset() {
    const nextValues = cloneValueRecord(this.#defaultValues as Record<string, unknown> | undefined)

    this.#dirtyFields.clear()

    for (const fieldName of this.#schema.keys) {
      this.#hydrateField(fieldName, nextValues)
    }

    this.values = nextValues
    this.errors = clearAllErrors<T>(this.errors, this.#schema.keys)
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
      const errors = buildFieldErrors<T>(this.#schema.keys, parsed.error)

      this.errors = errors
      this.commit()
      this.#onInvalid?.({
        errors,
        error: parsed.error as FormInvalidResult<T>["error"],
      })
      this.#focusFirstInvalidField(formElement, parsed.error)
      return
    }

    this.errors = clearAllErrors<T>(this.errors, this.#schema.keys)
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

  #hydrateField(fieldName: string, values: FormValueRecord) {
    hydrateField(
      values,
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

  #emitValues(nextValues: FormValueRecord) {
    if (!this.#onValuesChange) {
      return
    }

    if (areValueRecordsEqual(this.#lastEmittedValues, nextValues)) {
      return
    }

    const snapshot = cloneValueRecord(nextValues)
    this.#lastEmittedValues = snapshot
    this.#onValuesChange(snapshot as Partial<z.input<T>>)
  }
}

export interface FormController<T extends AnyObjectSchema> extends FormControllerState<T> {
  readonly [formControllerSchemaType]?: T
}

function createFormErrors<T extends AnyObjectSchema>(fieldNames: readonly string[]) {
  return createEmptyErrors(fieldNames) as FormErrors<T>
}

function clearFieldError<T extends AnyObjectSchema>(errors: FormErrors<T>, fieldName: string) {
  if ((errors as FieldErrorRecord)[fieldName] === null) {
    return errors
  }

  return {
    ...errors,
    [fieldName]: null,
  } as FormErrors<T>
}

function clearAllErrors<T extends AnyObjectSchema>(
  errors: FormErrors<T>,
  fieldNames: readonly string[],
) {
  if (Object.values(errors).every((message) => message === null)) {
    return errors
  }

  return createFormErrors<T>(fieldNames)
}

function buildFieldErrors<T extends AnyObjectSchema>(
  fieldNames: readonly string[],
  error: z.ZodError,
) {
  const nextErrors = createEmptyErrors(fieldNames)

  for (const issue of error.issues) {
    const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

    if (!fieldName || nextErrors[fieldName] !== null) {
      continue
    }

    nextErrors[fieldName] = issue.message
  }

  return nextErrors as FormErrors<T>
}
