import type { RJSFSchema } from "@rjsf/utils"
import { render } from "preact"
import { useState } from "preact/hooks"
import { act } from "preact/test-utils"
import { afterEach, expect, test, vi } from "vitest"

import { ConfigurationForm } from "./configuration-form.tsx"

function Icon(props: Record<string, unknown>) {
  return <svg {...props} />
}

vi.mock("lucide-react", () => ({
  ChevronDown: Icon,
  ChevronUp: Icon,
  Copy: Icon,
  Plus: Icon,
  Trash2: Icon,
  X: Icon,
}))

const schema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  type: "object",
  properties: {
    name: { $ref: "#/$defs/Name" },
    enabled: { type: "boolean" },
    untouched: { type: "string", default: "schema default" },
    items: {
      type: "array",
      items: { type: "string" },
    },
    nested: {
      title: "Nested settings",
      type: "object",
      properties: {
        value: { type: "string" },
      },
      additionalProperties: false,
    },
  },
  additionalProperties: false,
  $defs: {
    Name: { type: "string", minLength: 1 },
  },
} satisfies RJSFSchema

afterEach(() => {
  render(null, document.body)
  document.body.replaceChildren()
})

test("configuration form supports Preact, Draft 2020, and field commit timing", async () => {
  const onChange = vi.fn()

  function Harness() {
    const [document, setDocument] = useState({})
    return (
      <ConfigurationForm
        disabled={false}
        document={document}
        schema={schema}
        onDocumentChange={(nextDocument) => {
          setDocument(nextDocument)
          onChange(nextDocument)
        }}
      />
    )
  }

  await act(async () => {
    render(<Harness />, document.body)
  })

  const nameInput = requireInput("root_name")
  const untouchedInput = requireInput("root_untouched")
  const enabledInput = requireInput("root_enabled")

  expect(untouchedInput.value).toBe("")
  expect(onChange).not.toHaveBeenCalled()
  const nestedDetails = document.querySelector("details")
  expect(nestedDetails?.open).toBe(false)

  await act(async () => {
    nestedDetails?.querySelector("summary")?.click()
  })
  expect(nestedDetails?.open).toBe(true)

  await act(async () => {
    nameInput.value = "Goddard"
    nameInput.dispatchEvent(new Event("input", { bubbles: true }))
  })
  expect(onChange).not.toHaveBeenCalled()

  await act(async () => {
    nameInput.focus()
    nameInput.blur()
  })
  expect(onChange).toHaveBeenLastCalledWith({ name: "Goddard" })

  await act(async () => {
    enabledInput.checked = true
    enabledInput.dispatchEvent(new Event("change", { bubbles: true }))
  })
  expect(onChange).toHaveBeenLastCalledWith({ enabled: true, name: "Goddard" })

  const nestedInput = requireInput("root_nested_value")
  await act(async () => {
    nestedInput.value = "nested value"
    nestedInput.dispatchEvent(new Event("input", { bubbles: true }))
  })
  await act(async () => {
    nestedInput.focus()
    nestedInput.blur()
  })
  expect(onChange).toHaveBeenLastCalledWith({
    enabled: true,
    name: "Goddard",
    nested: { value: "nested value" },
  })
  expect(nestedDetails?.open).toBe(true)

  const addItems = document.querySelector(
    "button[aria-label='Add optional setting']",
  ) as HTMLButtonElement
  expect(addItems).toBeInstanceOf(HTMLButtonElement)

  await act(async () => {
    addItems.click()
  })
  expect(onChange).toHaveBeenLastCalledWith({
    enabled: true,
    items: [],
    name: "Goddard",
    nested: { value: "nested value" },
  })
  expect(document.querySelector("button[aria-label='Remove optional setting']")).toBeInstanceOf(
    HTMLButtonElement,
  )
})

function requireInput(id: string) {
  const input = document.getElementById(id)
  if (!(input instanceof HTMLInputElement)) {
    throw new Error(`Expected input ${id}.`)
  }
  return input
}
