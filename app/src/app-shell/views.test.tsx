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
