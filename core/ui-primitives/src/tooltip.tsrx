import { signal, type Signal } from "@preact/signals"
import { toChildArray, type VNode } from "preact"
import { useId, useRef } from "preact/hooks"

import { Popover } from "./popover.tsrx"
import { TooltipTrigger } from "./tooltip-trigger.ts"

/** Attaches unstyled tooltip behavior to a single trigger element. */
export function Tooltip(props: {
  open?: Signal<boolean>;
  ariaLabel?: string;
  ariaLabelledBy?: string;
  children: preact.ComponentChildren;
  class?: string;
  closeDelay?: number;
  content: preact.ComponentChildren;
  group?: string;
  id?: string;
  openDelay?: number;
  side?: "top" | "right" | "bottom" | "left";
  sideOffset?: number;
  style?: preact.CSSProperties;
}) @{
  const [trigger] = toChildArray(props.children)

  if (!trigger || typeof trigger !== "object" || !("props" in trigger)) {
    return null
  }

  const generatedTooltipId = useId()
  const tooltipId = props.id ?? generatedTooltipId
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
      ariaLabelledBy={props.ariaLabelledBy}
      class={props.class}
      closeOnEscape={true}
      closeOnOutsidePointer={false}
      focusOnOpen={false}
      group={null}
      id={tooltipId}
      offset={props.sideOffset ?? 8}
      {open}
      placement={props.side ?? "top"}
      restoreFocus={false}
      role="tooltip"
      strategy="fixed"
      style={props.style}
    >{props.content}</Popover>
  </>
}
