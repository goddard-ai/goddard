import { render } from "preact"
import { act } from "preact/test-utils"
import { afterEach, expect, test, vi } from "vitest"

import { useErrorBoundaryReset } from "./error-boundary-reset.ts"

function ErrorBoundaryResetHarness(props: { reset: () => void }) {
  useErrorBoundaryReset(props.reset)

  return null
}

afterEach(() => {
  vi.useRealTimers()
})

test("error boundary reset retries with exponential backoff", async () => {
  vi.useFakeTimers()
  const resets: number[] = []
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <ErrorBoundaryResetHarness
        reset={() => {
          resets.push(Date.now())
        }}
      />,
      container,
    )
  })

  vi.advanceTimersByTime(999)

  expect(resets).toHaveLength(0)

  vi.advanceTimersByTime(1)

  expect(resets).toHaveLength(1)

  vi.advanceTimersByTime(1999)

  expect(resets).toHaveLength(1)

  vi.advanceTimersByTime(1)

  expect(resets).toHaveLength(2)

  vi.advanceTimersByTime(4000)

  expect(resets).toHaveLength(3)

  render(null, container)
  container.remove()
})

test("error boundary reset retries immediately when the document gains focus", async () => {
  vi.useFakeTimers()
  const resets: number[] = []
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <ErrorBoundaryResetHarness
        reset={() => {
          resets.push(Date.now())
        }}
      />,
      container,
    )
  })

  vi.advanceTimersByTime(500)
  document.dispatchEvent(new FocusEvent("focus"))

  expect(resets).toHaveLength(1)

  vi.advanceTimersByTime(1999)

  expect(resets).toHaveLength(1)

  vi.advanceTimersByTime(1)

  expect(resets).toHaveLength(2)

  render(null, container)
  container.remove()
})

test("error boundary reset cancels retries after unmount", async () => {
  vi.useFakeTimers()
  const resets: number[] = []
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <ErrorBoundaryResetHarness
        reset={() => {
          resets.push(Date.now())
        }}
      />,
      container,
    )
  })

  render(null, container)
  vi.advanceTimersByTime(1000)

  expect(resets).toEqual([])

  container.remove()
})
