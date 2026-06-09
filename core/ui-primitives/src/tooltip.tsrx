import { signal, type Signal } from "@preact/signals"
import { toChildArray, type VNode } from "preact"
import { useId, useRef } from "preact/hooks"

import { Popover } from "./popover.tsrx"
import { TooltipTrigger } from "./tooltip-trigger.ts"

/** Attaches unstyled tooltip behavior to a single trigger element. */
export function Tooltip(props: {
  open?: Signal<boolean>;
  ariaLabel?: string;
  children: preact.ComponentChildren;
  class?: string;
  closeDelay?: number;
  content: preact.ComponentChildren;
  group?: string;
  openDelay?: number;
  side?: "top" | "right" | "bottom" | "left";
  sideOffset?: number;
}) @{
  const [trigger] = toChildArray(props.children)

  if (!trigger || typeof trigger !== "object" || !("props" in trigger)) {
    return null
  }

  const tooltipId = useId()
  const triggerRef = useRef<HTMLElement | null>(null)
  const openSignal = useRef<Signal<boolean> | null>(null)
  const open = props.open ?? (() => (openSignal.current ??= signal(false)))()

  <>
    <TooltipTrigger
      closeDelay={props.closeDelay}
      group={props.group}
      {open}
      openDelay={props.openDelay}
      {tooltipId}
      trigger={trigger as VNode}
      {triggerRef}
    />

    <Popover
      anchor={() => triggerRef.current}
      ariaLabel={props.ariaLabel}
      class={props.class}
      closeOnEscape={true}
      closeOnOutsidePointer={false}
      focusOnOpen={false}
      offset={props.sideOffset ?? 8}
      {open}
      placement={props.side ?? "top"}
      restoreFocus={false}
      role="tooltip"
      strategy="fixed"
    >{props.content}</Popover>
  </>
}
