import { signal } from "@preact/signals"
import { expect, test } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { Menu, MenuItem } from "../src/menu.tsrx"
import { setOverlayPortalRoots } from "../src/overlay/portal.ts"
import { Tooltip } from "../src/tooltip.tsrx"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

test("Menu forwards caller-owned DOM identity, labelling, and styles", async () => {
  const anchor = document.createElement("button")
  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const open = signal(true)

  document.body.append(anchor, menuRoot, container)
  setOverlayPortalRoots({
    menu: menuRoot,
  })

  function TestHarness() {
    return (
      <Menu
        anchor={() => anchor}
        ariaLabelledBy="actions-heading"
        id="actions-menu"
        open={open}
        style={{ color: "red" }}
      >
        <MenuItem id="run-action" style={{ background: "blue" }}>
          Run
        </MenuItem>
      </Menu>
    )
  }

  await act(async () => {
    render(<TestHarness />, container)
  })
  await flushEffects()

  const menu = menuRoot.querySelector("#actions-menu")
  const item = menuRoot.querySelector("#run-action")

  expect(menu).toBeInstanceOf(HTMLElement)
  expect(menu?.getAttribute("aria-labelledby")).toBe("actions-heading")
  expect((menu as HTMLElement | null)?.style.color).toBe("red")
  expect(item).toBeInstanceOf(HTMLElement)
  expect((item as HTMLElement | null)?.style.background).toBe("blue")

  render(null, container)
  anchor.remove()
  menuRoot.remove()
  container.remove()
})

test("Tooltip uses caller-owned content id, labelling, and styles", async () => {
  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const open = signal(true)

  document.body.append(menuRoot, container)
  setOverlayPortalRoots({
    menu: menuRoot,
  })

  function TestHarness() {
    return (
      <Tooltip
        ariaLabelledBy="tooltip-heading"
        content="Saved automatically"
        id="status-tooltip"
        open={open}
        style={{ color: "red" }}
      >
        <button type="button">Status</button>
      </Tooltip>
    )
  }

  await act(async () => {
    render(<TestHarness />, container)
  })
  await flushEffects()

  const trigger = container.querySelector("button")
  const tooltip = menuRoot.querySelector("#status-tooltip")

  expect(trigger?.getAttribute("aria-describedby")).toBe("status-tooltip")
  expect(tooltip).toBeInstanceOf(HTMLElement)
  expect(tooltip?.getAttribute("aria-labelledby")).toBe("tooltip-heading")
  expect((tooltip as HTMLElement | null)?.style.color).toBe("red")

  render(null, container)
  menuRoot.remove()
  container.remove()
})
