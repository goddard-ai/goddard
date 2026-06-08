import { expect, test } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { WorkbenchScrollPanel } from "./views.tsrx"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

test("WorkbenchScrollPanel focuses tab content when the active content changes", async () => {
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:first">
        <button>First tab action</button>
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  const firstPanel = container.querySelector("[tabindex='-1']")

  expect(document.activeElement).not.toBe(firstPanel)

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:second">
        <button>Second tab action</button>
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  const secondPanel = container.querySelector("[tabindex='-1']")

  expect(document.activeElement).toBe(secondPanel)

  render(null, container)
  container.remove()
})

test("WorkbenchScrollPanel focuses a search box when the active content changes", async () => {
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:first">
        <button>First tab action</button>
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:second">
        <input type="search" />
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  const searchInput = container.querySelector("input[type='search']")

  expect(document.activeElement).toBe(searchInput)

  render(null, container)
  container.remove()
})

test("WorkbenchScrollPanel focuses a search box on Mod+f", async () => {
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:search">
        <button>Other action</button>
        <input type="search" />
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  const searchInput = container.querySelector("input[type='search']")
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    key: "f",
    metaKey: true,
  })

  document.dispatchEvent(event)

  expect(event.defaultPrevented).toBe(true)
  expect(document.activeElement).toBe(searchInput)

  render(null, container)
  container.remove()
})

test("WorkbenchScrollPanel leaves Mod+f alone when no search box is available", async () => {
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:plain">
        <button>Only action</button>
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    key: "f",
    metaKey: true,
  })

  document.dispatchEvent(event)

  expect(event.defaultPrevented).toBe(false)

  render(null, container)
  container.remove()
})

test("WorkbenchScrollPanel focuses a delayed activation target", async () => {
  const container = document.createElement("div")
  document.body.append(container)

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:first">
        <button>First tab action</button>
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:second">
        <div />
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  await act(async () => {
    render(
      <WorkbenchScrollPanel scrollKey="detail:second">
        <div contentEditable={true} data-workbench-activation-focus="true" />
      </WorkbenchScrollPanel>,
      container,
    )
  })
  await flushEffects()

  const contentEditable = container.querySelector("[data-workbench-activation-focus='true']")

  expect(document.activeElement).toBe(contentEditable)

  render(null, container)
  container.remove()
})
