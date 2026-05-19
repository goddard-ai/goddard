import { Signal, signal } from "@preact/signals"
import { expect, test } from "bun:test"

import { lens } from "./lens.ts"

test("lens returns a writable signal projection", () => {
  const current = signal<"project" | "model" | null>("project")
  const modelOpen = lens(
    () => current.value === "model",
    (open) => {
      current.value = open ? "model" : null
    },
  )

  expect(modelOpen instanceof Signal).toBe(true)
  expect(modelOpen.value).toBe(false)

  modelOpen.value = true
  expect(current.value).toBe("model")
  expect(modelOpen.value).toBe(true)

  modelOpen.value = false
  expect(current.value).toBe(null)
  expect(modelOpen.value).toBe(false)
})

test("lens keeps computed signal subscriptions", () => {
  const count = signal(1)
  const doubled = lens(
    () => count.value * 2,
    (value) => {
      count.value = value / 2
    },
  )
  const values: number[] = []
  const unsubscribe = doubled.subscribe((value) => {
    values.push(value)
  })

  try {
    count.value = 2
    doubled.value = 10

    expect(values).toEqual([2, 4, 10])
  } finally {
    unsubscribe()
  }
})
