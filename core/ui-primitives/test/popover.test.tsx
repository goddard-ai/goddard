import { signal } from "@preact/signals"
import { expect, test } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { setOverlayPortalRoots } from "../src/overlay/portal.ts"
import { Popover } from "../src/popover.tsrx"

const menuPortalId = "menu-portal"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

function renderPopover(props: { closeOnOutsidePointer?: boolean }) {
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
        closeOnOutsidePointer={props.closeOnOutsidePointer}
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

test("Popover blocks outside pointer interactions when outside dismissal is enabled", async () => {
  const harness = renderPopover({})

  await harness.render()

  expect(harness.menuRoot.querySelector("[data-overlay-pointer-blocker='true']")).toBeInstanceOf(
    HTMLElement,
  )
  harness.cleanup()
})

test("Popover does not block outside pointer interactions when outside dismissal is disabled", async () => {
  const harness = renderPopover({ closeOnOutsidePointer: false })

  await harness.render()

  expect(harness.menuRoot.querySelector("[data-overlay-pointer-blocker='true']")).toBeNull()
  harness.cleanup()
})
