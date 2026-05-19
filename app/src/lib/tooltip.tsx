import { type Signal } from "@preact/signals"
import { cloneElement, Fragment, h, toChildArray, type VNode } from "preact"
import { useId, useRef } from "preact/hooks"

import { Popover } from "./popover.tsrx"

export type TooltipProps = {
  open: Signal<boolean>
  ariaLabel?: string
  children: preact.ComponentChildren
  class?: string
  closeDelay?: number
  content: preact.ComponentChildren
  openDelay?: number
  side?: "top" | "right" | "bottom" | "left"
  sideOffset?: number
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

export function Tooltip(props: TooltipProps) {
  const tooltipId = useId()
  const triggerRef = useRef<HTMLElement | null>(null)
  const openDelayRef = useRef<number | null>(null)
  const closeDelayRef = useRef<number | null>(null)
  const children = toChildArray(props.children)
  const trigger = children[0]

  if (!trigger || typeof trigger !== "object" || !("props" in trigger)) {
    return null
  }

  const triggerProps = trigger.props as Record<string, any>

  function openTooltip() {
    clearDelay(closeDelayRef)
    clearDelay(openDelayRef)
    openDelayRef.current = window.setTimeout(() => {
      props.open.value = true
      openDelayRef.current = null
    }, props.openDelay ?? 450)
  }

  function closeTooltip() {
    clearDelay(openDelayRef)
    clearDelay(closeDelayRef)
    closeDelayRef.current = window.setTimeout(() => {
      props.open.value = false
      closeDelayRef.current = null
    }, props.closeDelay ?? 80)
  }

  const triggerElement = cloneElement(trigger as VNode, {
    "aria-describedby": props.open.value ? tooltipId : triggerProps["aria-describedby"],
    ref(element: HTMLElement | null) {
      triggerRef.current = element

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

  return h(
    Fragment,
    {},
    triggerElement,
    h(
      Popover,
      {
        anchor: () => triggerRef.current,
        ariaLabel: props.ariaLabel,
        class: props.class,
        closeOnEscape: true,
        closeOnOutsidePointer: false,
        offset: props.sideOffset ?? 8,
        open: props.open,
        placement: props.side ?? "top",
        restoreFocus: false,
        role: "tooltip",
        strategy: "fixed",
      },
      props.content,
    ),
  )
}
