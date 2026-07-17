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

function setMaxTouchPoints(value: number) {
  const descriptor = Object.getOwnPropertyDescriptor(navigator, "maxTouchPoints")

  Object.defineProperty(navigator, "maxTouchPoints", { configurable: true, value })

  return () => {
    if (descriptor) {
      Object.defineProperty(navigator, "maxTouchPoints", descriptor)
    } else {
      delete (navigator as { maxTouchPoints?: number }).maxTouchPoints
    }
  }
}

function pressKey(controller: ListNavigationController, key: string) {
  const event = new KeyboardEvent("keydown", { key, cancelable: true })

  controller.onKeyDown(event)
  return event
}

function renderListNavigation(props: {
  activeAttribute?: string
  count: Signal<number>
  capture: (controller: ListNavigationController) => void
  disabledIndexes?: Set<number>
  onActivate?: (index: number) => void
  onActiveIndexChange?: (index: number) => void
  scrollIntoView?: boolean | ScrollIntoViewOptions
  shouldIgnorePointer?: () => boolean
  wrap?: boolean
}) {
  const container = document.createElement("div")

  document.body.append(container)

  function TestHarness() {
    const navigation = useListNavigation({
      activeAttribute: props.activeAttribute,
      count: () => props.count.value,
      onActivate: props.onActivate,
      onActiveIndexChange: props.onActiveIndexChange,
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
  activeAttribute?: string
  count: Signal<number>
  capture: (controller: SearchNavigationController) => void
  onActivate?: (index: number) => void
  onActiveIndexChange?: (index: number) => void
  onEscape?: () => void
  onQueryChange: (query: string) => void
}) {
  const container = document.createElement("div")

  document.body.append(container)

  function TestHarness() {
    const navigation = useSearchNavigation({
      activeAttribute: props.activeAttribute,
      count: () => props.count.value,
      onActivate: props.onActivate,
      onActiveIndexChange: props.onActiveIndexChange,
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

test("useListNavigation never exposes its activation target as selected on touch", async () => {
  const restoreMaxTouchPoints = setMaxTouchPoints(1)
  const activatedIndexes: number[] = []
  const count = signal(2)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    activeAttribute: "aria-selected",
    count,
    onActivate(index) {
      activatedIndexes.push(index)
    },
    capture(controller) {
      navigation = controller
    },
  })

  try {
    await harness.render()

    const buttons = harness.container.querySelectorAll("button")
    expect(navigation!.activeIndex()).toBe(0)
    expect([...buttons].map((button) => button.getAttribute("aria-selected"))).toEqual([
      "false",
      "false",
    ])

    pressKey(navigation!, "Enter")
    expect(activatedIndexes).toEqual([0])

    pressKey(navigation!, "ArrowDown")
    expect(navigation!.activeIndex()).toBe(1)
    expect([...buttons].map((button) => button.getAttribute("aria-selected"))).toEqual([
      "false",
      "false",
    ])

    navigation!.setActiveIndex(0)
    expect([...buttons].map((button) => button.getAttribute("aria-selected"))).toEqual([
      "false",
      "false",
    ])

    navigation!.resetActiveIndex()
    expect([...buttons].map((button) => button.getAttribute("aria-selected"))).toEqual([
      "false",
      "false",
    ])
  } finally {
    harness.cleanup()
    restoreMaxTouchPoints()
  }
})

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

test("useListNavigation reports active index changes from primitive-owned movement", async () => {
  const onActiveIndexChange = vi.fn()
  const count = signal(4)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    onActiveIndexChange,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  pressKey(navigation!, "ArrowDown")
  navigation!.setActiveIndex(3)

  const buttons = harness.container.querySelectorAll("button")
  buttons[2]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 1 }))

  count.value = 2
  await harness.render()

  navigation!.resetActiveIndex()
  navigation!.setActiveIndex(0)

  expect(onActiveIndexChange.mock.calls).toEqual([[1], [3], [2], [1], [0]])

  harness.cleanup()
})

test("useSearchNavigation wires input changes, reset, Enter activation, and Escape", async () => {
  const count = signal(3)
  const onActivate = vi.fn()
  const onEscape = vi.fn()
  const onQueryChange = vi.fn()
  let navigation: SearchNavigationController | null = null
  const harness = renderSearchNavigation({
    activeAttribute: "aria-selected",
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
  expect(
    [...harness.container.querySelectorAll("button")].map((button) =>
      button.getAttribute("aria-selected"),
    ),
  ).toEqual(["true", "false", "false"])

  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", cancelable: true }))
  expect(onActivate).toHaveBeenCalledWith(0)

  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }))
  expect(onEscape).toHaveBeenCalled()

  harness.cleanup()
})

test("useSearchNavigation reports active index changes from input handlers", async () => {
  const count = signal(3)
  const onActiveIndexChange = vi.fn()
  let navigation: SearchNavigationController | null = null
  const harness = renderSearchNavigation({
    count,
    onActiveIndexChange,
    onQueryChange() {},
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  const input = harness.container.querySelector("input")!

  input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", cancelable: true }))
  input.value = "abc"
  input.dispatchEvent(new InputEvent("input", { bubbles: true }))

  expect(navigation!.activeIndex()).toBe(0)
  expect(onActiveIndexChange.mock.calls).toEqual([[1], [0]])

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

test("useListNavigation can focus registered items and only highlights after pointer movement", async () => {
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

  buttons[1]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 1 }))
  expect(navigation!.activeIndex()).toBe(0)

  ignorePointer = false
  buttons[1]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 2, clientY: 1 }))
  expect(navigation!.activeIndex()).toBe(1)

  navigation!.focusActiveItem()
  expect(document.activeElement).toBe(buttons[1])

  navigation!.focusItem(0)
  expect(document.activeElement).toBe(buttons[0])

  harness.cleanup()
})

test("useListNavigation does not scroll when pointer movement highlights an item", async () => {
  const scrollIntoView = vi
    .spyOn(HTMLElement.prototype, "scrollIntoView")
    .mockImplementation(() => {})
  const count = signal(2)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()
  scrollIntoView.mockClear()

  const buttons = harness.container.querySelectorAll("button")

  buttons[1]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 1 }))

  expect(navigation!.activeIndex()).toBe(1)
  expect(buttons[1]?.getAttribute("data-highlighted")).toBe("true")
  expect(scrollIntoView).not.toHaveBeenCalled()

  await harness.render()
  expect(navigation!.activeIndex()).toBe(1)
  expect(scrollIntoView).not.toHaveBeenCalled()

  scrollIntoView.mockRestore()
  harness.cleanup()
})

test("useListNavigation ignores stale pointer enters after item updates until pointer movement", async () => {
  const count = signal(2)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  let buttons = harness.container.querySelectorAll("button")

  buttons[1]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 1 }))
  expect(navigation!.activeIndex()).toBe(1)

  count.value = 3
  await harness.render()
  buttons = harness.container.querySelectorAll("button")

  buttons[0]?.dispatchEvent(new PointerEvent("pointerenter"))
  expect(navigation!.activeIndex()).toBe(1)

  buttons[0]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 2 }))
  expect(navigation!.activeIndex()).toBe(0)

  harness.cleanup()
})

test("useListNavigation ignores stale pointer enters after scrolling until pointer movement", async () => {
  const count = signal(2)
  let navigation: ListNavigationController | null = null
  const harness = renderListNavigation({
    count,
    capture(controller) {
      navigation = controller
    },
  })

  await harness.render()

  const buttons = harness.container.querySelectorAll("button")

  buttons[1]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 1 }))
  expect(navigation!.activeIndex()).toBe(1)

  harness.container.dispatchEvent(new Event("scroll"))
  buttons[0]?.dispatchEvent(new PointerEvent("pointerenter"))
  expect(navigation!.activeIndex()).toBe(1)

  buttons[0]?.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 2 }))
  expect(navigation!.activeIndex()).toBe(0)

  harness.cleanup()
})
