import { setOverlayPortalRoots } from "@goddard-ai/ui-primitives"
import { render } from "preact"
import { act } from "preact/test-utils"
import { afterEach, expect, test, vi } from "vitest"

import { appToaster, createToaster, GoodToaster } from "./good-toaster.tsrx"

vi.mock("lucide-react", () => ({
  LoaderCircle: () => <svg aria-hidden="true" data-icon="loader" />,
  X: () => <svg aria-hidden="true" />,
}))

afterEach(() => {
  appToaster.dismissAll()
  vi.useRealTimers()
})

test("toaster auto-dismisses non-error toasts by default", () => {
  vi.useFakeTimers()
  const toaster = createToaster({ max: 4 })

  toaster.create({
    title: "Saved",
    type: "success",
  })

  expect(toaster.toasts.value).toHaveLength(1)

  vi.advanceTimersByTime(4000)

  expect(toaster.toasts.value).toHaveLength(0)
})

test("toaster keeps error toasts visible by default", () => {
  vi.useFakeTimers()
  const toaster = createToaster({ max: 4 })

  toaster.create({
    title: "Failed",
    type: "error",
  })

  vi.advanceTimersByTime(4000)

  expect(toaster.toasts.value.map((toast) => toast.title)).toEqual(["Failed"])
})

test("toaster keeps loading toasts visible by default", () => {
  vi.useFakeTimers()
  const toaster = createToaster({ max: 4 })

  toaster.create({
    title: "Loading",
    type: "loading",
  })

  vi.advanceTimersByTime(4000)

  expect(toaster.toasts.value.map((toast) => toast.title)).toEqual(["Loading"])
})

test("toaster updates an existing toast and reschedules dismissal", () => {
  vi.useFakeTimers()
  const toaster = createToaster({ max: 4 })

  const id = toaster.create({
    title: "Loading",
    type: "loading",
  })

  toaster.update(id, {
    title: "Saved",
    type: "success",
  })

  expect(toaster.toasts.value.map((toast) => toast.title)).toEqual(["Saved"])

  vi.advanceTimersByTime(4000)

  expect(toaster.toasts.value).toHaveLength(0)
})

test("toaster honors explicit durations for any toast type", () => {
  vi.useFakeTimers()
  const toaster = createToaster({ max: 4 })

  toaster.create({
    duration: 1200,
    title: "Failed briefly",
    type: "error",
  })

  vi.advanceTimersByTime(1199)

  expect(toaster.toasts.value).toHaveLength(1)

  vi.advanceTimersByTime(1)

  expect(toaster.toasts.value).toHaveLength(0)
})

test("toaster preserves visible errors when enforcing the visible cap", () => {
  const toaster = createToaster({ max: 3 })

  toaster.create({ id: "error-1", title: "Error 1", type: "error" })
  toaster.create({ id: "info-1", title: "Info 1", type: "info" })
  toaster.create({ id: "error-2", title: "Error 2", type: "error" })
  toaster.create({ id: "success-1", title: "Success 1", type: "success" })

  expect(toaster.toasts.value.map((toast) => toast.id)).toEqual(["error-1", "error-2", "success-1"])
})

test("toaster dismisses the oldest error when every visible toast is an error", () => {
  const toaster = createToaster({ max: 2 })

  toaster.create({ id: "error-1", title: "Error 1", type: "error" })
  toaster.create({ id: "error-2", title: "Error 2", type: "error" })
  toaster.create({ id: "error-3", title: "Error 3", type: "error" })

  expect(toaster.toasts.value.map((toast) => toast.id)).toEqual(["error-2", "error-3"])
})

test("toaster dismisses individual toasts and all toasts", () => {
  const toaster = createToaster({ max: 4 })

  toaster.create({ id: "info-1", title: "Info 1" })
  toaster.create({ id: "info-2", title: "Info 2" })

  toaster.dismiss("info-1")

  expect(toaster.toasts.value.map((toast) => toast.id)).toEqual(["info-2"])

  toaster.dismissAll()

  expect(toaster.toasts.value).toEqual([])
})

test("GoodToaster renders a polite live region with close buttons", async () => {
  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  menuRoot.id = "menu-portal"
  document.body.append(container, menuRoot)
  setOverlayPortalRoots({
    menu: menuRoot,
  })

  appToaster.create({ id: "info-1", title: "Saved" })

  await act(async () => {
    render(<GoodToaster />, container)
  })

  expect(menuRoot.querySelector("[aria-live='polite']")?.textContent).toContain("Saved")

  const closeButton = menuRoot.querySelector("button")
  closeButton?.click()

  await act(async () => {
    await Promise.resolve()
  })

  expect(appToaster.toasts.value).toEqual([])

  render(null, container)
  container.remove()
  menuRoot.remove()
})
