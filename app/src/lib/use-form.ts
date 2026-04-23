import { useEffect, useRef, useState } from "preact/hooks"
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
  const optionsRef = useRef(options)
  const dirtyFieldsRef = useRef(new Set<string>())
  const draftValuesRef = useRef<FormValueRecord>(cloneValueRecord(options.initialValues))
  const lastEmittedValuesRef = useRef<FormValueRecord>(cloneValueRecord(options.initialValues))
  const previousInitialValuesRef = useRef(options.initialValues)
  const controlsByFieldRef = useRef(new Map<string, Set<FormControl>>())
  const fieldNameByControlRef = useRef(new WeakMap<FormControl, string>())
  const cleanupByControlRef = useRef(new WeakMap<FormControl, () => void>())
  const submittingRef = useRef(false)
  const refsRef = useRef<FormRefRecord | null>(null)
  const [errors, setErrors] = useState<FieldErrorRecord>(() => createEmptyErrors(schema.keys))
  const [isSubmitting, setIsSubmitting] = useState(false)

  optionsRef.current = options

  function getFieldControls(fieldName: string, formElement?: HTMLFormElement) {
    sweepDisconnectedControls(fieldName)

    const fieldControls = [...(controlsByFieldRef.current.get(fieldName) ?? [])].filter(
      (control) => !formElement || formElement.contains(control),
    )

    fieldControls.sort(compareControlsByDomOrder)
    return fieldControls
  }

  function setDraftFieldValue(fieldName: string, fieldValue: unknown) {
    const nextValues = cloneValueRecord(draftValuesRef.current)
    assignRecordValue(nextValues, fieldName, fieldValue)
    draftValuesRef.current = nextValues
    return nextValues
  }

  function emitChange(nextValues: FormValueRecord) {
    if (!optionsRef.current.onChange) {
      return
    }

    if (areValueRecordsEqual(lastEmittedValuesRef.current, nextValues)) {
      return
    }

    const snapshot = cloneValueRecord(nextValues)
    lastEmittedValuesRef.current = snapshot
    optionsRef.current.onChange(snapshot as Partial<z.input<T>>)
  }

  function clearFieldError(fieldName: string) {
    setErrors((currentErrors) => {
      if (currentErrors[fieldName] === null) {
        return currentErrors
      }

      return {
        ...currentErrors,
        [fieldName]: null,
      }
    })
  }

  function clearAllErrors() {
    setErrors((currentErrors) => {
      if (Object.values(currentErrors).every((message) => message === null)) {
        return currentErrors
      }

      return createEmptyErrors(schema.keys)
    })
  }

  function syncFieldValueFromDom(fieldName: string) {
    const nextFieldValue = readFieldValue(getFieldControls(fieldName))
    return setDraftFieldValue(fieldName, nextFieldValue)
  }

  function handleFieldChange(fieldName: string) {
    dirtyFieldsRef.current.add(fieldName)
    clearFieldError(fieldName)
    emitChange(syncFieldValueFromDom(fieldName))
  }

  function observeControl(fieldName: string, control: FormControl) {
    if (cleanupByControlRef.current.has(control)) {
      return
    }

    const eventName = getObservedEventName(control)
    const handleControlChange = () => {
      handleFieldChange(fieldName)
    }

    control.addEventListener(eventName, handleControlChange)
    cleanupByControlRef.current.set(control, () => {
      control.removeEventListener(eventName, handleControlChange)
    })
  }

  /**
   * Restores the current stored field value into every connected control for that field. This
   * keeps uncontrolled remounts and late pristine hydration aligned with the persisted draft.
   */
  function hydrateField(fieldName: string) {
    writeFieldValue(getFieldControls(fieldName), draftValuesRef.current[fieldName])
  }

  function removeControl(fieldName: string, control: FormControl) {
    controlsByFieldRef.current.get(fieldName)?.delete(control)
    cleanupByControlRef.current.get(control)?.()
    cleanupByControlRef.current.delete(control)
    fieldNameByControlRef.current.delete(control)
  }

  function sweepDisconnectedControls(fieldName: string) {
    for (const control of controlsByFieldRef.current.get(fieldName) ?? []) {
      if (control.isConnected) {
        continue
      }

      removeControl(fieldName, control)
    }
  }

  function registerFieldControl(fieldName: string, control: FormControl) {
    const previousFieldName = fieldNameByControlRef.current.get(control)

    if (previousFieldName && previousFieldName !== fieldName) {
      removeControl(previousFieldName, control)
    }

    control.name = fieldName
    observeControl(fieldName, control)

    const fieldControls = controlsByFieldRef.current.get(fieldName) ?? new Set<FormControl>()
    fieldControls.add(control)
    controlsByFieldRef.current.set(fieldName, fieldControls)
    fieldNameByControlRef.current.set(control, fieldName)

    hydrateField(fieldName)
  }

  function createFieldRefs() {
    const nextRefs: FormRefRecord = {}

    for (const fieldName of schema.keys) {
      nextRefs[fieldName] = (node) => {
        if (node) {
          registerFieldControl(fieldName, node)
          return
        }

        sweepDisconnectedControls(fieldName)
      }
    }

    return nextRefs
  }

  if (!refsRef.current) {
    refsRef.current = createFieldRefs()
  }

  function focusFirstInvalidField(error: z.ZodError, formElement: HTMLFormElement) {
    for (const issue of error.issues) {
      const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

      if (!fieldName) {
        continue
      }

      const control = getFieldControls(fieldName, formElement)[0]

      if (control) {
        control.focus()
        return
      }
    }
  }

  function buildFieldErrors(error: z.ZodError) {
    const nextErrors = createEmptyErrors(schema.keys)

    for (const issue of error.issues) {
      const fieldName = typeof issue.path[0] === "string" ? issue.path[0] : null

      if (!fieldName || nextErrors[fieldName] !== null) {
        continue
      }

      nextErrors[fieldName] = issue.message
    }

    return nextErrors
  }

  /**
   * Reads the live values for the controls inside the submitted form instead of relying on stored
   * draft state, so hidden or unmounted fields are not silently submitted.
   */
  function collectSubmissionValues(formElement: HTMLFormElement) {
    const formControls = [...formElement.elements].filter(
      (control): control is FormControl =>
        isFormControl(control) && fieldNameByControlRef.current.has(control),
    )
    const submissionValues: FormValueRecord = {}

    for (const fieldName of schema.keys) {
      assignRecordValue(
        submissionValues,
        fieldName,
        readFieldValue(formControls.filter((control) => control.name === fieldName)),
      )
    }

    return submissionValues
  }

  function clear() {
    const nextValues: FormValueRecord = {}

    for (const fieldName of schema.keys) {
      dirtyFieldsRef.current.add(fieldName)

      const fieldControls = getFieldControls(fieldName)
      writeFieldValue(fieldControls, getEmptyFieldValue(fieldControls))
      assignRecordValue(nextValues, fieldName, readFieldValue(fieldControls))
    }

    draftValuesRef.current = nextValues
    clearAllErrors()
    emitChange(nextValues)
  }

  async function submit(event: preact.TargetedSubmitEvent<HTMLFormElement>) {
    event.preventDefault()

    if (submittingRef.current) {
      return
    }

    const submissionValues = collectSubmissionValues(event.currentTarget)
    const parsed = schema.zod.safeParse(submissionValues)

    if (!parsed.success) {
      setErrors(buildFieldErrors(parsed.error))
      optionsRef.current.onInvalid?.(parsed.error)
      focusFirstInvalidField(parsed.error, event.currentTarget)
      return
    }

    clearAllErrors()
    submittingRef.current = true
    setIsSubmitting(true)

    try {
      await optionsRef.current.onSubmit(parsed.data)
    } finally {
      submittingRef.current = false
      setIsSubmitting(false)
    }
  }

  useEffect(() => {
    if (previousInitialValuesRef.current === options.initialValues) {
      return
    }

    previousInitialValuesRef.current = options.initialValues
    const nextInitialValues = cloneValueRecord(options.initialValues)

    for (const fieldName of schema.keys) {
      if (dirtyFieldsRef.current.has(fieldName)) {
        continue
      }

      setDraftFieldValue(fieldName, nextInitialValues[fieldName])
      hydrateField(fieldName)
    }
  }, [options.initialValues, schema])

  return {
    refs: refsRef.current as FormRefs<T>,
    errors: errors as FormErrors<T>,
    isSubmitting,
    submit,
    clear,
  }
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
