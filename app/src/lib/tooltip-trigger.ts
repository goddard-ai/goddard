import { type Signal } from "@preact/signals"
import { cloneElement, type VNode } from "preact"
import { useRef } from "preact/hooks"

/** Wires tooltip accessibility and pointer/focus behavior onto the caller-provided trigger vnode. */
export function TooltipTrigger(props: {
  closeDelay?: number
  open: Signal<boolean>
  openDelay?: number
  tooltipId: string
  trigger: VNode
  triggerRef: preact.RefObject<HTMLElement | null>
}) {
  const triggerProps = props.trigger.props as Record<string, any>
  const openDelayRef = useRef<number | null>(null)
  const closeDelayRef = useRef<number | null>(null)
  const openTooltip = () => {
    clearDelay(closeDelayRef)
    clearDelay(openDelayRef)
    openDelayRef.current = window.setTimeout(() => {
      props.open.value = true
      openDelayRef.current = null
    }, props.openDelay ?? 450)
  }
  const closeTooltip = () => {
    clearDelay(openDelayRef)
    clearDelay(closeDelayRef)
    closeDelayRef.current = window.setTimeout(() => {
      props.open.value = false
      closeDelayRef.current = null
    }, props.closeDelay ?? 80)
  }

  return cloneElement(props.trigger, {
    "aria-describedby": props.open.value ? props.tooltipId : triggerProps["aria-describedby"],
    ref(element: HTMLElement | null) {
      props.triggerRef.current = element

      if (typeof triggerProps.ref === "function") {
        triggerProps.ref(element)
      } else if (triggerProps.ref && typeof triggerProps.ref === "object") {
        triggerProps.ref.current = element
      }
    },
    onBlur: chainHandlers(triggerProps.onBlur, closeTooltip),
    onFocus: chainHandlers(triggerProps.onFocus, openTooltip),
    onKeyDown: chainHandlers(triggerProps.onKeyDown, (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        props.open.value = false
      }
    }),
    onPointerDown: chainHandlers(triggerProps.onPointerDown, () => {
      props.open.value = false
    }),
    onPointerEnter: chainHandlers(triggerProps.onPointerEnter, openTooltip),
    onPointerLeave: chainHandlers(triggerProps.onPointerLeave, closeTooltip),
  })
}

function chainHandlers(first: ((event: any) => void) | undefined, second: (event: any) => void) {
  return (event: any) => {
    first?.(event)
    second(event)
  }
}

function clearDelay(delay: preact.RefObject<number | null>) {
  if (delay.current !== null) {
    window.clearTimeout(delay.current)
    delay.current = null
  }
}
