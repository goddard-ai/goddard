import { signal } from "@preact/signals"
import { expect, test } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { setOverlayPortalRoots } from "../src/overlay/portal.ts"
import { Popover, type PopoverCloseReason } from "../src/popover.tsrx"

const menuPortalId = "menu-portal"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

function renderPopover(props: {
  ariaLabelledBy?: string
  closeOnOutsidePointer?: boolean
  id?: string
  restoreFocus?: boolean
  style?: preact.CSSProperties
}) {
  const anchor = document.createElement("button")
  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const open = signal(true)

  menuRoot.id = menuPortalId
  document.body.append(anchor, menuRoot, container)
  setOverlayPortalRoots({
    menu: menuRoot,
  })

  function TestHarness() {
    return (
      <Popover
        anchor={() => anchor}
        open={open}
        ariaLabelledBy={props.ariaLabelledBy}
        closeOnOutsidePointer={props.closeOnOutsidePointer}
        id={props.id}
        restoreFocus={props.restoreFocus}
        style={props.style}
      >
        <button>Inside</button>
      </Popover>
    )
  }

  return {
    container,
    menuRoot,
    open,
    cleanup() {
      render(null, container)
      anchor.remove()
      menuRoot.remove()
      container.remove()
    },
    async render() {
      await act(async () => {
        render(<TestHarness />, container)
      })
      await flushEffects()
    },
  }
}

function renderGroupedPopovers(props: { group?: string | null }) {
  const firstAnchor = document.createElement("button")
  const secondAnchor = document.createElement("button")
  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const firstOpen = signal(true)
  const secondOpen = signal(true)
  const closed: PopoverCloseReason[] = []

  menuRoot.id = menuPortalId
  document.body.append(firstAnchor, secondAnchor, menuRoot, container)
  setOverlayPortalRoots({
    menu: menuRoot,
  })

  function TestHarness() {
    return (
      <>
        <Popover
          anchor={() => firstAnchor}
          group={props.group}
          open={firstOpen}
          onOpenChange={(_open, reason) => {
            closed.push(reason)
          }}
        >
          <button>First</button>
        </Popover>
        <Popover anchor={() => secondAnchor} group={props.group} open={secondOpen}>
          <button>Second</button>
        </Popover>
      </>
    )
  }

  return {
    closed,
    container,
    firstOpen,
    menuRoot,
    secondOpen,
    cleanup() {
      render(null, container)
      firstAnchor.remove()
      secondAnchor.remove()
      menuRoot.remove()
      container.remove()
    },
    async render() {
      await act(async () => {
        render(<TestHarness />, container)
      })
      await flushEffects()
    },
  }
}

test("Popover blocks outside pointer interactions when outside dismissal is enabled", async () => {
  const harness = renderPopover({})

  await harness.render()

  expect(harness.menuRoot.querySelector(".goddard-overlay-pointer-blocker")).toBeInstanceOf(
    HTMLElement,
  )
  harness.cleanup()
})

test("Popover applies caller-owned DOM identity, labelling, and styles", async () => {
  const harness = renderPopover({
    ariaLabelledBy: "popover-heading",
    id: "custom-popover",
    style: { color: "red" },
  })

  await harness.render()

  const popover = harness.menuRoot.querySelector("#custom-popover")
  expect(popover).toBeInstanceOf(HTMLElement)
  expect(popover?.getAttribute("aria-labelledby")).toBe("popover-heading")
  expect((popover as HTMLElement | null)?.style.color).toBe("red")
  harness.cleanup()
})

test("Popover does not block outside pointer interactions when outside dismissal is disabled", async () => {
  const harness = renderPopover({ closeOnOutsidePointer: false })

  await harness.render()

  expect(harness.menuRoot.querySelector(".goddard-overlay-pointer-blocker")).toBeNull()
  harness.cleanup()
})

test("Popover leaves focus alone when restoreFocus is disabled", async () => {
  const harness = renderPopover({ restoreFocus: false })
  const nextButton = document.createElement("button")

  document.body.append(nextButton)
  await harness.render()

  nextButton.focus()
  harness.open.value = false
  await harness.render()

  expect(document.activeElement).toBe(nextButton)

  nextButton.remove()
  harness.cleanup()
})

test("Popover closes earlier default-group popovers", async () => {
  const harness = renderGroupedPopovers({})

  await harness.render()

  expect(harness.firstOpen.value).toBe(false)
  expect(harness.secondOpen.value).toBe(true)
  expect(harness.closed).toEqual(["group"])
  harness.cleanup()
})

test("Popover allows multiple unmanaged popovers", async () => {
  const harness = renderGroupedPopovers({ group: null })

  await harness.render()

  expect(harness.firstOpen.value).toBe(true)
  expect(harness.secondOpen.value).toBe(true)
  expect(harness.closed).toEqual([])
  harness.cleanup()
})

test("Escape closes only the topmost unmanaged popover", async () => {
  const harness = renderGroupedPopovers({ group: null })

  await harness.render()

  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }))
  await harness.render()

  expect(harness.firstOpen.value).toBe(true)
  expect(harness.secondOpen.value).toBe(false)
  harness.cleanup()
})
