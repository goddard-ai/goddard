import { Modal } from "@goddard-ai/ui-primitives"
import { signal } from "@preact/signals"

const open = signal(false)

/** Demonstrates a labelled modal with explicit close ownership in application code. */
export function ModalExample() {
  return (
    <>
      <button onClick={() => (open.value = true)}>Open settings</button>
      <Modal
        open={open}
        titleId="settings-title"
        descriptionId="settings-description"
        backdropClass="modal-backdrop"
        positionerClass="modal-positioner"
        contentClass="modal-content"
      >
        <h2 id="settings-title">Settings</h2>
        <p id="settings-description">Update workspace settings.</p>
        <button onClick={() => (open.value = false)}>Close</button>
      </Modal>
    </>
  )
}
