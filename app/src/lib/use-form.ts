import { useSigma } from "preact-sigma"
import { useEffect, useRef } from "preact/hooks"
import { z } from "zod"

import { createFieldRefs } from "./use-form/dom.ts"
import { FormManager, FormCallbacksRuntime, FormRuntime } from "./use-form/manager.ts"
import {
  createForm,
  type AnyObjectSchema,
  type FormErrors,
  type FormRefs,
  type FormSchema,
} from "./use-form/schema.ts"
import type { FormRefRecord } from "./use-form/types.ts"
import { createEmptyErrors, cloneValueRecord } from "./use-form/values.ts"

export { createForm } from "./use-form/schema.ts"

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
