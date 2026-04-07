/** Shared, pre-styled tooltip primitives for app UI chrome. */
import * as Tooltip from "@radix-ui/react-tooltip"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { ComponentChildren } from "preact"

const tooltipContentClass = css({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  pointerEvents: "none",
  padding: "8px 12px",
  borderRadius: "12px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  color: "text",
  fontSize: "0.8rem",
  fontWeight: "620",
  lineHeight: "1",
  boxShadow: "0 18px 40px rgba(121, 138, 160, 0.16)",
  transformOrigin: "var(--radix-tooltip-content-transform-origin)",
  transition:
    "opacity 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)",
  zIndex: "10",
  "@starting-style": {
    opacity: "0",
    transform: "scale(0.97)",
  },
})

const tooltipArrowClass = css({
  fill: token.var("colors.background"),
  stroke: token.var("colors.border"),
  strokeWidth: "1",
})

/** Provides shared tooltip timing and exclusivity across the app shell. */
export function GoodTooltipProvider(props: { children: ComponentChildren }) {
  return (
    <Tooltip.Provider delayDuration={450} disableHoverableContent={true} skipDelayDuration={80}>
      {props.children}
    </Tooltip.Provider>
  )
}

/** Attaches the shared app tooltip treatment to one trigger element. */
export function GoodTooltip(props: {
  ariaLabel?: string
  children: ComponentChildren
  content: ComponentChildren
  side?: "top" | "right" | "bottom" | "left"
  sideOffset?: number
}) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>{props.children}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          aria-label={props.ariaLabel}
          class={tooltipContentClass}
          side={props.side ?? "top"}
          sideOffset={props.sideOffset ?? 8}
        >
          {props.content}
          <Tooltip.Arrow class={tooltipArrowClass} height={6} width={10} />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
