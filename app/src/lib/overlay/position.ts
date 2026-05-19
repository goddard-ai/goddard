import {
  autoUpdate,
  computePosition,
  flip,
  offset,
  shift,
  size,
  type Placement,
  type Strategy,
} from "@floating-ui/dom"

/** Placement settings shared by anchored overlay primitives. */
export type FloatingPositionOptions = {
  placement?: Placement
  offset?: number
  sameWidth?: boolean
  strategy?: Strategy
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
      strategy: options.strategy ?? "absolute",
      middleware: [
        offset(options.offset ?? 4),
        flip(),
        shift({ padding: 8 }),
        size({
          padding: 8,
          apply({ availableHeight, availableWidth, rects }) {
            Object.assign(floatingElement.style, {
              "--available-height": `${availableHeight}px`,
              "--available-width": `${availableWidth}px`,
              "--reference-width": `${rects.reference.width}px`,
              width: options.sameWidth ? `${rects.reference.width}px` : "",
            })
          },
        }),
      ],
    })

    if (disposed) {
      return
    }

    Object.assign(floatingElement.style, {
      left: `${x}px`,
      position: options.strategy ?? "absolute",
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
