import type { SigmaRef } from "preact-sigma"
import { z } from "zod"

import type { AnyObjectSchema, FormControl, FormSchema } from "./schema.ts"

export type FormValueRecord = Record<string, unknown>
export type FieldErrorRecord = Record<string, string | null>
export type FormRefRecord = Record<string, (node: FormControl | null) => void>
export type FormCallbacksState = {
  onChange?(values: FormValueRecord): void
  onInvalid?(error: z.ZodError): void
  onSubmit(values: unknown): void | Promise<void>
}
export type FormRuntimeState = {
  controlsByField: Map<string, Set<FormControl>>
  fieldNameByControl: WeakMap<FormControl, string>
  cleanupByControl: WeakMap<FormControl, () => void>
  dirtyFields: Set<string>
  lastEmittedValues: FormValueRecord
  previousInitialValues: Record<string, unknown> | undefined
}
export type FormManagerShape = {
  schema: SigmaRef<FormSchema<AnyObjectSchema>>
  callbacks: SigmaRef<FormCallbacksState>
  runtime: SigmaRef<FormRuntimeState>
  draftValues: FormValueRecord
  errors: FieldErrorRecord
  isSubmitting: boolean
}
export type FormStateAccess = {
  readonly schema: FormSchema<AnyObjectSchema>
  readonly callbacks: FormCallbacksState
  readonly runtime: FormRuntimeState
  readonly draftValues: FormValueRecord
}
export type MutableFormErrorState = FormStateAccess & {
  errors: FieldErrorRecord
}
export type FormRefState = FormStateAccess & {
  attachFieldControl(fieldName: string, control: FormControl): void
  handleFieldChange(fieldName: string): void
  sweepDisconnectedControls(fieldName: string): void
}
