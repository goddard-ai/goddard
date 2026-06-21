import { signal } from "@preact/signals"
import { expect, test, vi } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

import { Modal } from "../src/modal.tsrx"
import { setOverlayPortalRoots } from "../src/overlay/portal.ts"

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

function renderModal(props: {
  backdropStyle?: preact.CSSProperties
  closeOnOutsidePointer?: boolean
  contentId?: string
  contentStyle?: preact.CSSProperties
  onBeforeClose?: (reason: "escape" | "explicit" | "outside") => boolean
  positionerStyle?: preact.CSSProperties
}) {
  const container = document.createElement("div")
  const trigger = document.createElement("button")
  const dialogRoot = document.createElement("div")
  const open = signal(true)

  dialogRoot.id = "dialog-portal"
  document.body.append(trigger, dialogRoot, container)
  setOverlayPortalRoots({
    dialog: dialogRoot,
  })
  trigger.focus()

  function TestHarness() {
    return (
      <Modal
        open={open}
        titleId="modal-title"
        descriptionId="modal-description"
        backdropStyle={props.backdropStyle}
        closeOnOutsidePointer={props.closeOnOutsidePointer}
        contentId={props.contentId}
        contentStyle={props.contentStyle}
        initialFocus={() => document.getElementById("modal-input") as HTMLInputElement | null}
        onBeforeClose={props.onBeforeClose}
        positionerStyle={props.positionerStyle}
      >
        <h2 id="modal-title">Title</h2>
        <p id="modal-description">Description</p>
        <input id="modal-input" />
      </Modal>
    )
  }

  return {
    cleanup() {
      render(null, container)
      trigger.remove()
      dialogRoot.remove()
      container.remove()
    },
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
  harness.cleanup()
})

test("Modal applies caller-owned DOM identity and styles to modal surfaces", async () => {
  const harness = renderModal({
    backdropStyle: { background: "rgba(0, 0, 0, 0.5)" },
    contentId: "settings-dialog",
    contentStyle: { color: "red" },
    positionerStyle: { display: "grid" },
  })

  await harness.render()

  const backdrop = harness.dialogRoot.querySelector('[aria-hidden="true"]')
  const positioner = harness.dialogRoot.querySelector('[style*="display"]')
  const content = harness.dialogRoot.querySelector("#settings-dialog")

  expect((backdrop as HTMLElement | null)?.style.background).toBe("rgba(0, 0, 0, 0.5)")
  expect((positioner as HTMLElement | null)?.style.display).toBe("grid")
  expect((content as HTMLElement | null)?.style.color).toBe("red")
  harness.cleanup()
})

test("Modal does not close on outside pointer by default", async () => {
  const harness = renderModal({})

  await harness.render()

  document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }))
  await flushEffects()

  expect(harness.open.value).toBe(true)
  harness.cleanup()
})

test("Modal can block close attempts for confirmation flows", async () => {
  const onBeforeClose = vi.fn(() => false)
  const harness = renderModal({ onBeforeClose })

  await harness.render()

  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }))
  await flushEffects()

  expect(onBeforeClose).toHaveBeenCalledWith("escape")
  expect(harness.open.value).toBe(true)
  harness.cleanup()
})
