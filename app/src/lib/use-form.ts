import { type SigmaRef, SigmaType, useSigma } from "preact-sigma"
import { useEffect, useRef } from "preact/hooks"
import { z } from "zod"

type FormControl = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
type AnyObjectSchema = z.ZodObject<z.ZodRawShape>
type FieldName<T extends AnyObjectSchema> = keyof z.input<T> & string
type FormErrors<T extends AnyObjectSchema> = {
  readonly [K in FieldName<T>]: string | null
}
type FormRefs<T extends AnyObjectSchema> = {
  readonly [K in FieldName<T>]: (node: FormControl | null) => void
}
type FormSchema<T extends AnyObjectSchema> = {
  readonly zod: T
  readonly keys: readonly FieldName<T>[]
}
type FormValueRecord = Record<string, unknown>
type FieldErrorRecord = Record<string, string | null>
type FormRefRecord = Record<string, (node: FormControl | null) => void>
type FormCallbacks = {
  onChange?(values: FormValueRecord): void
  onInvalid?(error: z.ZodError): void
  onSubmit(values: unknown): void | Promise<void>
}
class FormCallbacksRuntime {
  onChange?: (values: FormValueRecord) => void
  onInvalid?: (error: z.ZodError) => void
  onSubmit: (values: unknown) => void | Promise<void> = () => {}

  sync(nextCallbacks: FormCallbacks) {
    this.onChange = nextCallbacks.onChange
    this.onInvalid = nextCallbacks.onInvalid
    this.onSubmit = nextCallbacks.onSubmit
  }
}
class FormRuntime {
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
type FormManagerShape = {
  schema: SigmaRef<FormSchema<AnyObjectSchema>>
  callbacks: SigmaRef<FormCallbacksRuntime>
  runtime: SigmaRef<FormRuntime>
  draftValues: FormValueRecord
  errors: FieldErrorRecord
  isSubmitting: boolean
}
type FormStateAccess = {
  readonly schema: FormSchema<AnyObjectSchema>
  readonly callbacks: FormCallbacksRuntime
  readonly runtime: FormRuntime
  readonly draftValues: FormValueRecord
}
type MutableFormErrorState = FormStateAccess & {
  errors: FieldErrorRecord
}

/**
 * Creates a flat Zod-backed form descriptor with stable field keys for the hook runtime.
 */
export function createForm<const TShape extends z.ZodRawShape>(shape: TShape) {
  const zod = z.object(shape)

  return {
    zod,
    keys: Object.freeze(Object.keys(shape)) as readonly (keyof TShape & string)[],
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

type FormManagerState = typeof FormManager.Instance

/**
 * Registers uncontrolled inputs against one flat schema, parses on submit, and surfaces keyed
 * refs and keyed field errors.
 */
export function useForm<const T extends AnyObjectSchema>(
  schema: FormSchema<T>,
  options: {
    initialValues?: Partial<z.input<T>>
    onChange?(values: Partial<z.input<T>>): void
    onInvalid?(error: z.ZodError): void
    onSubmit(values: z.output<T>): void | Promise<void>
  },
) {
  const callbacksRef = useRef<FormCallbacksRuntime>(new FormCallbacksRuntime())
  const refsRef = useRef<FormRefRecord | null>(null)
  callbacksRef.current.sync({
    onChange: options.onChange
      ? (values) => {
          options.onChange?.(values as Partial<z.input<T>>)
        }
      : undefined,
    onInvalid: options.onInvalid,
    onSubmit: (values) => {
      return options.onSubmit(values as z.output<T>)
    },
  })
  const form = useSigma(
    () =>
      new FormManager({
        schema,
        callbacks: callbacksRef.current,
        runtime: new FormRuntime(options.initialValues),
        draftValues: cloneValueRecord(options.initialValues),
        errors: createEmptyErrors(schema.keys),
      }),
  )

  if (!refsRef.current) {
    refsRef.current = createFieldRefs(form, schema.keys)
  }

  useEffect(() => {
    form.syncInitialValues(options.initialValues)
  }, [form, options.initialValues])

  return {
    refs: refsRef.current as FormRefs<T>,
    errors: form.errors as FormErrors<T>,
    isSubmitting: form.isSubmitting,
    submit(event: preact.TargetedSubmitEvent<HTMLFormElement>) {
      event.preventDefault()
      void form.submit(event.currentTarget)
    },
    clear() {
      form.clear()
    },
  }
}

function createFieldRefs(form: FormManagerState, fieldNames: readonly string[]) {
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

function observeControl(form: FormManagerState, fieldName: string, control: FormControl) {
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

function collectSubmissionValues(form: FormStateAccess, formElement: HTMLFormElement) {
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

function focusFirstInvalidField(
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

function hydrateField(form: FormStateAccess, fieldName: string) {
  writeFieldValue(getFieldControls(form, fieldName), form.draftValues[fieldName])
}

function getFieldControls(form: FormStateAccess, fieldName: string, formElement?: HTMLFormElement) {
  sweepDisconnectedControls(form, fieldName)

  const fieldControls = [...(form.runtime.controlsByField.get(fieldName) ?? [])].filter(
    (control) => !formElement || formElement.contains(control),
  )

  fieldControls.sort(compareControlsByDomOrder)
  return fieldControls
}

function sweepDisconnectedControls(form: FormStateAccess, fieldName: string) {
  for (const control of form.runtime.controlsByField.get(fieldName) ?? []) {
    if (control.isConnected) {
      continue
    }

    detachFieldControl(form, fieldName, control)
  }
}

function detachFieldControl(form: FormStateAccess, fieldName: string, control: FormControl) {
  form.runtime.controlsByField.get(fieldName)?.delete(control)
  form.runtime.cleanupByControl.get(control)?.()
  form.runtime.cleanupByControl.delete(control)
  form.runtime.fieldNameByControl.delete(control)
}

function createEmptyErrors(fieldNames: readonly string[]) {
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
function cloneValueRecord(values?: Record<string, unknown>) {
  const snapshot: FormValueRecord = {}

  for (const [fieldName, fieldValue] of Object.entries(values ?? {})) {
    snapshot[fieldName] = Array.isArray(fieldValue) ? [...fieldValue] : fieldValue
  }

  return snapshot
}

function setDraftFieldValue(values: FormValueRecord, fieldName: string, fieldValue: unknown) {
  const nextValues = cloneValueRecord(values)
  assignRecordValue(nextValues, fieldName, fieldValue)
  return nextValues
}

function assignRecordValue(values: FormValueRecord, fieldName: string, fieldValue: unknown) {
  if (fieldValue === undefined) {
    delete values[fieldName]
    return
  }

  values[fieldName] = Array.isArray(fieldValue) ? [...fieldValue] : fieldValue
}

function areValueRecordsEqual(leftValues: FormValueRecord, rightValues: FormValueRecord) {
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

/**
 * Serializes one flat field from its registered controls, with DOM-native coercion for the common
 * uncontrolled input types the hook owns.
 */
function readFieldValue(fieldControls: readonly FormControl[]) {
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

function getEmptyFieldValue(fieldControls: readonly FormControl[]) {
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
function writeFieldValue(fieldControls: readonly FormControl[], fieldValue: unknown) {
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
