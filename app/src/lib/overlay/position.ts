import { autoUpdate, computePosition, flip, offset, shift, type Placement } from "@floating-ui/dom"

/** Placement settings shared by anchored overlay primitives. */
export type FloatingPositionOptions = {
  placement?: Placement
  offset?: number
}

/** Keeps a floating overlay positioned against its anchor until either element unmounts. */
export function startFloatingPosition(
  referenceElement: HTMLElement,
  floatingElement: HTMLElement,
  options: FloatingPositionOptions = {},
) {
  let disposed = false

  async function updatePosition() {
    const { x, y } = await computePosition(referenceElement, floatingElement, {
      placement: options.placement ?? "bottom-start",
      middleware: [offset(options.offset ?? 4), flip(), shift({ padding: 8 })],
    })

    if (disposed) {
      return
    }

    Object.assign(floatingElement.style, {
      left: `${x}px`,
      position: "absolute",
      top: `${y}px`,
    })
  }

  const stopAutoUpdate = autoUpdate(referenceElement, floatingElement, updatePosition)
  void updatePosition()

  return () => {
    disposed = true
    stopAutoUpdate()
  }
}
