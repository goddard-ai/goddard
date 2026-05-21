import { type Signal } from "@preact/signals"
import { cloneElement, type VNode } from "preact"
import { useEffect, useRef } from "preact/hooks"

type TooltipGroupState = {
  pendingCloseCallbacks: Set<() => void>
}

const tooltipGroups = new Map<string, TooltipGroupState>()

/** Wires tooltip accessibility and pointer/focus behavior onto the caller-provided trigger vnode. */
export function TooltipTrigger(props: {
  closeDelay?: number
  group?: string
  open: Signal<boolean>
  openDelay?: number
  tooltipId: string
  trigger: VNode
  triggerRef: preact.RefObject<HTMLElement | null>
}) {
  const triggerProps = props.trigger.props as Record<string, any>
  const openDelayRef = useRef<number | null>(null)
  const closeDelayRef = useRef<number | null>(null)
  const groupCloseCallbackRef = useRef<(() => void) | null>(null)
  const getGroupState = () => {
    if (!props.group) {
      return null
    }

    let groupState = tooltipGroups.get(props.group)
    if (!groupState) {
      groupState = {
        pendingCloseCallbacks: new Set(),
      }
      tooltipGroups.set(props.group, groupState)
    }

    return groupState
  }
  const removePendingGroupClose = () => {
    if (props.group && groupCloseCallbackRef.current) {
      tooltipGroups.get(props.group)?.pendingCloseCallbacks.delete(groupCloseCallbackRef.current)
      groupCloseCallbackRef.current = null
    }
  }
  const closeTooltipImmediately = () => {
    clearDelay(openDelayRef)
    clearDelay(closeDelayRef)
    removePendingGroupClose()
    props.open.value = false
  }
  const openTooltipImmediately = () => {
    clearDelay(closeDelayRef)
    clearDelay(openDelayRef)
    removePendingGroupClose()
    props.open.value = true
  }
  const openTooltip = () => {
    const groupState = getGroupState()
    if (groupState && groupState.pendingCloseCallbacks.size > 0) {
      for (const closePendingTooltip of [...groupState.pendingCloseCallbacks]) {
        closePendingTooltip()
      }

      openTooltipImmediately()
      return
    }

    clearDelay(closeDelayRef)
    clearDelay(openDelayRef)
    removePendingGroupClose()
    openDelayRef.current = window.setTimeout(() => {
      props.open.value = true
      openDelayRef.current = null
    }, props.openDelay ?? 450)
  }
  const closeTooltip = () => {
    clearDelay(openDelayRef)
    clearDelay(closeDelayRef)
    removePendingGroupClose()

    groupCloseCallbackRef.current = closeTooltipImmediately
    getGroupState()?.pendingCloseCallbacks.add(closeTooltipImmediately)
    closeDelayRef.current = window.setTimeout(() => {
      closeTooltipImmediately()
    }, props.closeDelay ?? 80)
  }

  useEffect(() => closeTooltipImmediately, [])

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
    onPointerDown: chainHandlers(triggerProps.onPointerDown, closeTooltipImmediately),
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
