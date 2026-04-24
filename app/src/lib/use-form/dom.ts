import type { FormControl } from "./schema.ts"
import type { FormRefRecord } from "./types.ts"
import { getObservedEventName } from "./values.ts"

type FormRefController = {
  runtime: {
    cleanupByControl: WeakMap<FormControl, () => void>
  }
  attachFieldControl(fieldName: string, control: FormControl): void
  handleFieldChange(fieldName: string): void
  sweepDisconnectedControls(fieldName: string): void
}

export function createFieldRefs(form: FormRefController, fieldNames: readonly string[]) {
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

function observeControl(form: FormRefController, fieldName: string, control: FormControl) {
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
