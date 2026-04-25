import { useSigma } from "preact-sigma"
import { useEffect, useRef } from "preact/hooks"
import { z } from "zod"

import { createFieldRefs } from "./use-form/dom.ts"
import { FormManager } from "./use-form/manager.ts"
import {
  createForm,
  type AnyObjectSchema,
  type FormErrors,
  type FormInvalidResult,
  type FormRefs,
  type FormSchema,
} from "./use-form/schema.ts"
import type { FormRefRecord } from "./use-form/types.ts"

export { createForm } from "./use-form/schema.ts"

/**
 * Registers uncontrolled inputs against one flat schema, parses on submit, and surfaces keyed
 * refs and keyed field errors.
 */
export function useForm<const T extends AnyObjectSchema>(
  schema: FormSchema<T>,
  options: {
    initialValues?: Partial<z.input<T>>
    onInvalid?(result: FormInvalidResult<T>): void
    onSubmit(values: z.output<T>): void | Promise<void>
    onValues?(values: Partial<z.input<T>>): void
  },
) {
  const optionsRef = useRef(options)
  const refsRef = useRef<FormRefRecord | null>(null)

  optionsRef.current = options

  const form = useSigma(
    () =>
      new FormManager({
        schema,
        initialValues: options.initialValues,
        onInvalid: (result) => {
          optionsRef.current.onInvalid?.(result)
        },
        onSubmit: (values) => {
          return optionsRef.current.onSubmit(values)
        },
        onValues: (values) => {
          optionsRef.current.onValues?.(values)
        },
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
    get errors() {
      return form.errors as FormErrors<T>
    },
    get isSubmitting() {
      return form.isSubmitting
    },
    submit(event: preact.TargetedSubmitEvent<HTMLFormElement>) {
      event.preventDefault()
      void form.submit(event.currentTarget)
    },
    clear() {
      form.clear()
    },
    reset() {
      form.reset()
    },
  }
}
