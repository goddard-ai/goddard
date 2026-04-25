import { useSigma } from "preact-sigma"
import { useEffect, useRef } from "preact/hooks"
import { z } from "zod"

import { FormManager } from "./use-form/manager.ts"
import {
  createForm,
  type AnyObjectSchema,
  type FormInvalidResult,
  type FormSchema,
} from "./use-form/schema.ts"

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

  useEffect(() => {
    form.syncInitialValues(options.initialValues)
  }, [form, options.initialValues])

  return form
}
