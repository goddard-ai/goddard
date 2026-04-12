/** Shared, pre-styled tooltip primitives for app UI chrome. */
import { Portal } from "@ark-ui/react/portal"
import { Tooltip } from "@ark-ui/react/tooltip"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { ComponentChildren } from "preact"

const tooltipArrowClass = css({
  width: "10px",
  height: "6px",
})

const tooltipArrowTipClass = css({
  width: "10px",
  height: "6px",
  background: token.var("colors.background"),
  borderTop: "1px solid",
  borderLeft: "1px solid",
  borderColor: "border",
  transform: "rotate(45deg) translateY(1px)",
})

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
  opacity: "1",
  transform: "scale(1)",
  transition:
    "opacity 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)",
  zIndex: "10",
  "@starting-style": {
    opacity: "0",
    transform: "scale(0.97)",
  },
})

/** Provides shared tooltip timing and exclusivity across the app shell. */
export function GoodTooltipProvider(props: { children: ComponentChildren }) {
  return props.children
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
    <Tooltip.Root
      closeDelay={80}
      closeOnPointerDown={false}
      openDelay={450}
      positioning={{
        gutter: props.sideOffset ?? 8,
        placement: props.side ?? "top",
      }}
    >
      <Tooltip.Trigger asChild>{props.children}</Tooltip.Trigger>
      <Portal>
        <Tooltip.Positioner>
          <Tooltip.Content aria-label={props.ariaLabel} class={tooltipContentClass}>
            {props.content}
            <Tooltip.Arrow class={tooltipArrowClass}>
              <Tooltip.ArrowTip class={tooltipArrowTipClass} />
            </Tooltip.Arrow>
          </Tooltip.Content>
        </Tooltip.Positioner>
      </Portal>
    </Tooltip.Root>
  )
}
