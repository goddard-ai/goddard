import { signal } from "@preact/signals"
import { expect, test, vi } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { Modal } from "./modal.tsrx"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

function renderModal(props: {
  closeOnOutsidePointer?: boolean
  onBeforeClose?: (reason: "escape" | "explicit" | "outside") => boolean
}) {
  const container = document.createElement("div")
  const trigger = document.createElement("button")
  const dialogRoot = document.createElement("div")
  const open = signal(true)

  dialogRoot.id = "dialog-portal"
  document.body.append(trigger, dialogRoot, container)
  trigger.focus()

  function TestHarness() {
    return (
      <Modal
        open={open}
        titleId="modal-title"
        descriptionId="modal-description"
        closeOnOutsidePointer={props.closeOnOutsidePointer}
        initialFocus={() => document.getElementById("modal-input") as HTMLInputElement | null}
        onBeforeClose={props.onBeforeClose}
      >
        <h2 id="modal-title">Title</h2>
        <p id="modal-description">Description</p>
        <input id="modal-input" />
      </Modal>
    )
  }

  return {
    container,
    dialogRoot,
    open,
    trigger,
    async render() {
      await act(async () => {
        render(<TestHarness />, container)
      })
      await flushEffects()
    },
  }
}

test("Modal focuses initial control and restores focus when Escape closes", async () => {
  const harness = renderModal({})

  await harness.render()

  expect(document.activeElement?.id).toBe("modal-input")

  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }))
  await flushEffects()

  expect(harness.open.value).toBe(false)
  expect(document.activeElement).toBe(harness.trigger)
})

test("Modal does not close on outside pointer by default", async () => {
  const harness = renderModal({})

  await harness.render()

  document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }))
  await flushEffects()

  expect(harness.open.value).toBe(true)
})

test("Modal can block close attempts for confirmation flows", async () => {
  const onBeforeClose = vi.fn(() => false)
  const harness = renderModal({ onBeforeClose })

  await harness.render()

  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }))
  await flushEffects()

  expect(onBeforeClose).toHaveBeenCalledWith("escape")
  expect(harness.open.value).toBe(true)
})
