import type { FormControl } from "./schema.ts"

export type FormValueRecord = Record<string, unknown>
export type FieldErrorRecord = Record<string, string | null>
export type FormRefRecord = Record<string, (node: FormControl | null) => void>
