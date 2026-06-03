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

export type FloatingPoint = {
  x: number
  y: number
}

export type FloatingReference = Element | FloatingPoint

/** Placement settings shared by anchored overlay primitives. */
export type FloatingPositionOptions = {
  placement?: Placement
  offset?: number
  sameWidth?: boolean
  strategy?: Strategy
}

/** Keeps a floating overlay positioned against its anchor until either element unmounts. */
export function startFloatingPosition(
  referenceElement: FloatingReference,
  floatingElement: HTMLElement,
  options: FloatingPositionOptions = {},
) {
  let disposed = false
  const reference =
    referenceElement instanceof Element
      ? referenceElement
      : {
          getBoundingClientRect() {
            return new DOMRect(referenceElement.x, referenceElement.y, 0, 0)
          },
        }

  async function updatePosition() {
    Object.assign(floatingElement.style, {
      maxHeight: "",
      maxWidth: "",
      minHeight: "",
      minWidth: "",
    })

    const { x, y } = await computePosition(reference, floatingElement, {
      placement: options.placement ?? "bottom-start",
      strategy: options.strategy ?? "absolute",
      middleware: [
        offset(options.offset ?? 4),
        flip(),
        shift({ padding: 8 }),
        size({
          padding: 8,
          apply({ availableHeight, availableWidth, rects }) {
            const boundedAvailableHeight = Math.max(0, availableHeight)
            const boundedAvailableWidth = Math.max(0, availableWidth)

            floatingElement.style.setProperty("--available-height", `${boundedAvailableHeight}px`)
            floatingElement.style.setProperty("--available-width", `${boundedAvailableWidth}px`)
            floatingElement.style.setProperty("--reference-width", `${rects.reference.width}px`)

            const floatingStyle = getComputedStyle(floatingElement)
            const cssMaxHeight = parseCssPixelSize(floatingStyle.maxHeight)
            const cssMaxWidth = parseCssPixelSize(floatingStyle.maxWidth)
            const cssMinHeight = parseCssPixelSize(floatingStyle.minHeight)
            const cssMinWidth = parseCssPixelSize(floatingStyle.minWidth)

            Object.assign(floatingElement.style, {
              maxHeight: `${Math.min(boundedAvailableHeight, cssMaxHeight ?? boundedAvailableHeight)}px`,
              maxWidth: `${Math.min(boundedAvailableWidth, cssMaxWidth ?? boundedAvailableWidth)}px`,
              minHeight:
                cssMinHeight !== null && cssMinHeight > boundedAvailableHeight
                  ? `${boundedAvailableHeight}px`
                  : "",
              minWidth:
                cssMinWidth !== null && cssMinWidth > boundedAvailableWidth
                  ? `${boundedAvailableWidth}px`
                  : "",
              width: options.sameWidth
                ? `${Math.min(rects.reference.width, boundedAvailableWidth)}px`
                : "",
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

  const stopAutoUpdate =
    reference instanceof Element
      ? autoUpdate(reference, floatingElement, updatePosition)
      : (() => {
          window.addEventListener("resize", updatePosition)
          window.addEventListener("scroll", updatePosition, true)

          return () => {
            window.removeEventListener("resize", updatePosition)
            window.removeEventListener("scroll", updatePosition, true)
          }
        })()
  void updatePosition()

  return () => {
    disposed = true
    stopAutoUpdate()
  }
}

function parseCssPixelSize(value: string) {
  if (!value.endsWith("px")) {
    return null
  }

  const size = Number.parseFloat(value)
  return Number.isFinite(size) ? size : null
}
