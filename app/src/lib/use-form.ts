import { useSigma } from "preact-sigma"
import { useEffect, useRef } from "preact/hooks"
import { z } from "zod"

import { FormController } from "./use-form/controller.ts"
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
    defaultValues?: Partial<z.input<T>>
    onInvalid?(result: FormInvalidResult<T>): void
    onSubmit(values: z.output<T>): void | Promise<void>
    onValuesChange?(values: Partial<z.input<T>>): void
  },
) {
  const optionsRef = useRef(options)

  optionsRef.current = options

  const form = useSigma(
    () =>
      new FormController({
        schema,
        defaultValues: options.defaultValues,
        onInvalid: (result) => {
          optionsRef.current.onInvalid?.(result)
        },
        onSubmit: (values) => {
          return optionsRef.current.onSubmit(values)
        },
        onValuesChange: (values) => {
          optionsRef.current.onValuesChange?.(values)
        },
      }),
  )

  useEffect(() => {
    form.syncDefaultValues(options.defaultValues)
  }, [form, options.defaultValues])

  return form
}
