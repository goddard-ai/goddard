import { expect, test, vi } from "bun:test"
import { render } from "preact"
import { useEffect } from "preact/hooks"
import { act } from "preact/test-utils"
import { z } from "zod"

import { createForm, useForm } from "./use-form.ts"

const SignupForm = createForm({
  name: z.string().trim().min(1, "Name is required"),
})

function useSignupForm(options: {
  initialValues?: { name?: string }
  onInvalid?(result: { error: z.ZodError; errors: { name: string | null } }): void
  onSubmit(values: { name: string }): void | Promise<void>
  onValues?(values: { name?: string }): void
}) {
  return useForm(SignupForm, options)
}

function createDeferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve
  })

  return {
    promise,
    resolve,
  }
}

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

function createHarness() {
  const container = document.createElement("div")
  document.body.append(container)

  let latestForm: ReturnType<typeof useSignupForm> | null = null
  let currentProps: Omit<Parameters<typeof TestHarness>[0], "onRender"> | null = null

  function TestHarness(props: {
    initialValues?: { name?: string }
    onInvalid?(result: { error: z.ZodError; errors: { name: string | null } }): void
    onRender(form: ReturnType<typeof useSignupForm>): void
    onSubmit(values: { name: string }): void | Promise<void>
    onValues?(values: { name?: string }): void
  }) {
    const form = useSignupForm({
      initialValues: props.initialValues,
      onInvalid: props.onInvalid,
      onSubmit: props.onSubmit,
      onValues: props.onValues,
    })

    useEffect(() => {
      props.onRender(form)
    })

    return (
      <form onSubmit={form.submit}>
        <input ref={form.refs.name} />
        <button type="submit">Submit</button>
      </form>
    )
  }

  return {
    container,
    get form() {
      if (!latestForm) {
        throw new Error("Expected form API to be available.")
      }

      return latestForm
    },
    get formElement() {
      const formElement = container.querySelector("form")

      if (!(formElement instanceof HTMLFormElement)) {
        throw new Error("Expected a form element.")
      }

      return formElement
    },
    get input() {
      const input = container.querySelector("input")

      if (!(input instanceof HTMLInputElement)) {
        throw new Error("Expected an input element.")
      }

      return input
    },
    async render(props: Omit<Parameters<typeof TestHarness>[0], "onRender">) {
      currentProps = props

      await act(async () => {
        render(
          <TestHarness
            {...props}
            onRender={(form) => {
              latestForm = form
            }}
          />,
          container,
        )
      })

      await flushEffects()
    },
    async rerender(nextProps: Partial<Omit<Parameters<typeof TestHarness>[0], "onRender">>) {
      if (!currentProps) {
        throw new Error("Expected harness to be rendered before rerendering.")
      }

      await this.render({
        ...currentProps,
        ...nextProps,
      })
    },
  }
}

test("initialValues hydrate the uncontrolled input and onValues receives raw values", async () => {
  const onValues = vi.fn()
  const harness = createHarness()

  await harness.render({
    initialValues: { name: "John" },
    onValues,
    onSubmit() {},
  })

  expect(harness.input.value).toBe("John")

  await act(async () => {
    harness.input.value = "Jane"
    harness.input.dispatchEvent(new Event("input", { bubbles: true }))
  })

  expect(onValues).toHaveBeenCalledTimes(1)
  expect(onValues).toHaveBeenLastCalledWith({ name: "Jane" })
})

test("submit parses the current values and keeps isSubmitting true until the promise settles", async () => {
  const submitDeferred = createDeferred<void>()
  const onSubmit = vi.fn(() => submitDeferred.promise)
  const harness = createHarness()

  await harness.render({
    initialValues: { name: "John" },
    onSubmit,
  })

  await act(async () => {
    harness.input.value = "  Jane  "
    harness.input.dispatchEvent(new Event("input", { bubbles: true }))
  })

  await act(async () => {
    harness.formElement.dispatchEvent(
      new SubmitEvent("submit", { bubbles: true, cancelable: true }),
    )
    await Promise.resolve()
  })

  expect(onSubmit).toHaveBeenCalledTimes(1)
  expect(onSubmit).toHaveBeenCalledWith({ name: "Jane" })
  expect(harness.form.isSubmitting).toBe(true)

  await act(async () => {
    submitDeferred.resolve()
    await submitDeferred.promise
  })

  expect(harness.form.isSubmitting).toBe(false)
})

test("invalid submit exposes keyed errors, reports them, and clears them when the field changes", async () => {
  const onInvalid = vi.fn()
  const harness = createHarness()

  await harness.render({
    onInvalid,
    onSubmit() {},
  })

  await act(async () => {
    harness.formElement.dispatchEvent(
      new SubmitEvent("submit", { bubbles: true, cancelable: true }),
    )
    await Promise.resolve()
  })

  expect(harness.form.errors.name).toBe("Name is required")
  expect(onInvalid).toHaveBeenCalledTimes(1)
  expect(onInvalid).toHaveBeenLastCalledWith({
    error: expect.any(z.ZodError),
    errors: {
      name: "Name is required",
    },
  })

  await act(async () => {
    harness.input.value = "Jane"
    harness.input.dispatchEvent(new Event("input", { bubbles: true }))
  })

  expect(harness.form.errors.name).toBe(null)
})

test("clear resets the uncontrolled input and emits the emptied values", async () => {
  const onValues = vi.fn()
  const harness = createHarness()

  await harness.render({
    initialValues: { name: "John" },
    onValues,
    onSubmit() {},
  })

  await act(async () => {
    harness.form.clear()
    await Promise.resolve()
  })

  expect(harness.input.value).toBe("")
  expect(onValues).toHaveBeenCalledTimes(1)
  expect(onValues).toHaveBeenLastCalledWith({ name: "" })
})

test("reset restores the latest initialValues and emits the restored values", async () => {
  const onValues = vi.fn()
  const harness = createHarness()

  await harness.render({
    initialValues: { name: "John" },
    onSubmit() {},
    onValues,
  })

  await act(async () => {
    harness.input.value = "Jane"
    harness.input.dispatchEvent(new Event("input", { bubbles: true }))
  })

  await harness.rerender({
    initialValues: { name: "Jill" },
  })

  await act(async () => {
    harness.form.reset()
    await Promise.resolve()
  })

  expect(harness.input.value).toBe("Jill")
  expect(onValues).toHaveBeenLastCalledWith({ name: "Jill" })
})

test("new initialValues rehydrate pristine fields but do not overwrite dirty ones", async () => {
  const harness = createHarness()

  await harness.render({
    initialValues: { name: "John" },
    onSubmit() {},
  })

  await harness.rerender({
    initialValues: { name: "Jane" },
  })

  expect(harness.input.value).toBe("Jane")

  await act(async () => {
    harness.input.value = "Jack"
    harness.input.dispatchEvent(new Event("input", { bubbles: true }))
  })

  await harness.rerender({
    initialValues: { name: "Jill" },
  })

  expect(harness.input.value).toBe("Jack")
})

test("createForm rejects nested field inputs", () => {
  expect(() =>
    createForm({
      profile: z.object({
        name: z.string(),
      }),
    }),
  ).toThrow('createForm only supports flat field inputs. "profile" requires nested input values.')
})
