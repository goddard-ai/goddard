import type { FormControl } from "./schema.ts"
import type { FormRefRecord } from "./types.ts"

type FormRefController = {
  attachFieldControl(fieldName: string, control: FormControl): void
  sweepDisconnectedControls(fieldName: string): void
}

export function createFieldRefs(form: FormRefController, fieldNames: readonly string[]) {
  const nextRefs: FormRefRecord = {}

  for (const fieldName of fieldNames) {
    nextRefs[fieldName] = (node) => {
      if (node) {
        form.attachFieldControl(fieldName, node)
        return
      }

      form.sweepDisconnectedControls(fieldName)
    }
  }

  return nextRefs
}
