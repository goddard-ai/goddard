import { signal, type Signal } from "@preact/signals"
import { expect, test, vi } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import {
  useListNavigation,
  useSearchNavigation,
  type ListNavigationController,
  type SearchNavigationController,
} from "../src/list-navigation.ts"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

function pressKey(controller: ListNavigationController, key: string) {
  const event = new KeyboardEvent("keydown", { key, cancelable: true })

  controller.onKeyDown(event)
  return event
}

function renderListNavigation(props: {
  count: Signal<number>
  capture: (controller: ListNavigationController) => void
  disabledIndexes?: Set<number>
  onActivate?: (index: number) => void
  scrollIntoView?: boolean | ScrollIntoViewOptions
  shouldIgnorePointer?: () => boolean
  wrap?: boolean
}) {
  const container = document.createElement("div")

  document.body.append(container)

  function TestHarness() {
    const navigation = useListNavigation({
      count: () => props.count.value,
      onActivate: props.onActivate,
      scrollIntoView: props.scrollIntoView,
      shouldIgnorePointer: props.shouldIgnorePointer,
      wrap: props.wrap,
    })

    props.capture(navigation)

    return (
      <div>
        {Array.from({ length: props.count.value }, (_, index) => (
          <button
            key={index}
            ref={navigation.itemRef(index)}
            aria-disabled={props.disabledIndexes?.has(index) ? "true" : undefined}
            type="button"
          >
            {index}
          </button>
        ))}
      </div>
    )
  }

  return {
    cleanup() {
      render(null, container)
      container.remove()
    },
    container,
    async render() {
      await act(async () => {
        render(<TestHarness />, container)
      })
      await flushEffects()
    },
  }
}

function renderSearchNavigation(props: {
  count: Signal<number>
  capture: (controller: SearchNavigationController) => void
  onActivate?: (index: number) => void
  onEscape?: () => void
  onQueryChange: (query: string) => void
}) {
  const container = document.createElement("div")

  document.body.append(container)

  function TestHarness() {
    const navigation = useSearchNavigation({
      count: () => props.count.value,
      onActivate: props.onActivate,
      onEscape: props.onEscape,
      onQueryChange: props.onQueryChange,
    })

    props.capture(navigation)

    return (
      <div>
        <input ref={navigation.inputRef} />
        {Array.from({ length: props.count.value }, (_, index) => (
          <button key={index} ref={navigation.itemRef(index)} type="button">
            {index}
          </button>
        ))}
      </div>
    )
  }

  return {
    cleanup() {
      render(null, container)
      container.remove()
    },
    container,
    async render() {
      await act(async () => {
        render(<TestHarness />, container)
      })
      await flushEffects()
    },
  }
}

test("useListNavigation wraps by default and scrolls the active item into view", async () => {
  const scrollIntoView = vi
    .spyOn(HTMLElement.prototype, "scrollIntoView")
    .mockImplementation(() => {})
  const count = signal(3)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()
  scrollIntoView.mockClear()

  pressKey(navigation!, "ArrowUp")

  const buttons = harness.container.querySelectorAll("button")
  expect(navigation!.activeIndex()).toBe(2)
  expect(buttons[2]?.getAttribute("data-highlighted")).toBe("true")
  expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest" })

  scrollIntoView.mockRestore()
  harness.cleanup()
})

test("useListNavigation skips disabled rows and does not activate them", async () => {
  const onActivate = vi.fn()
  const count = signal(3)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    disabledIndexes: new Set([1]),
    onActivate,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  pressKey(navigation!, "ArrowDown")
  expect(navigation!.activeIndex()).toBe(2)

  navigation!.setActiveIndex(1)
  expect(navigation!.activeIndex()).toBe(2)

  pressKey(navigation!, "Enter")
  expect(onActivate).toHaveBeenCalledWith(2)

  harness.cleanup()
})

test("useListNavigation clamps active index when count shrinks", async () => {
  const count = signal(4)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  navigation!.setActiveIndex(3)
  count.value = 2
  await harness.render()

  const buttons = harness.container.querySelectorAll("button")
  expect(navigation!.activeIndex()).toBe(1)
  expect(buttons[1]?.getAttribute("data-highlighted")).toBe("true")

  harness.cleanup()
})

test("useSearchNavigation wires input changes, reset, Enter activation, and Escape", async () => {
  const count = signal(3)
  const onActivate = vi.fn()
  const onEscape = vi.fn()
  const onQueryChange = vi.fn()
  let navigation: SearchNavigationController | null = null
  const harness = renderSearchNavigation({
    count,
    onActivate,
    onEscape,
    onQueryChange,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  const input = harness.container.querySelector("input")!

  navigation!.setActiveIndex(2)
  input.value = "abc"
  input.dispatchEvent(new InputEvent("input", { bubbles: true }))

  expect(onQueryChange).toHaveBeenCalledWith("abc")
  expect(navigation!.activeIndex()).toBe(0)

  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", cancelable: true }))
  expect(onActivate).toHaveBeenCalledWith(0)

  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }))
  expect(onEscape).toHaveBeenCalled()

  harness.cleanup()
})

test("useListNavigation supports ref cleanup functions and null cleanup", async () => {
  const count = signal(1)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  const element = document.createElement("button")
  const cleanup = navigation!.itemRef(0)(element)

  expect(element.getAttribute("data-highlighted")).toBe("true")

  if (cleanup) {
    cleanup()
  }

  element.setAttribute("data-highlighted", "false")
  element.dispatchEvent(new PointerEvent("pointerenter"))
  expect(element.getAttribute("data-highlighted")).toBe("false")

  navigation!.itemRef(0)(element)
  navigation!.itemRef(0)(null)
  element.setAttribute("data-highlighted", "false")
  element.dispatchEvent(new PointerEvent("pointerenter"))
  expect(element.getAttribute("data-highlighted")).toBe("false")

  harness.cleanup()
})

test("useListNavigation can focus registered items and suppress pointer highlighting", async () => {
  const count = signal(2)
  let ignorePointer = true
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
    shouldIgnorePointer: () => ignorePointer,
  })

  await harness.render()

  const buttons = harness.container.querySelectorAll("button")

  buttons[1]?.dispatchEvent(new PointerEvent("pointerenter"))
  expect(navigation!.activeIndex()).toBe(0)

  ignorePointer = false
  buttons[1]?.dispatchEvent(new PointerEvent("pointerenter"))
  expect(navigation!.activeIndex()).toBe(1)

  navigation!.focusActiveItem()
  expect(document.activeElement).toBe(buttons[1])

  navigation!.focusItem(0)
  expect(document.activeElement).toBe(buttons[0])

  harness.cleanup()
})
