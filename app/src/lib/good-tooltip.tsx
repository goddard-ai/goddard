/** Shared, pre-styled tooltip primitives for app UI chrome. */
import { Portal } from "@ark-ui/react/portal"
import { Tooltip } from "@ark-ui/react/tooltip"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

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

/** Attaches the shared app tooltip treatment to one trigger element. */
export function GoodTooltip(props: {
  ariaLabel?: string
  children: preact.ComponentChildren
  content: preact.ComponentChildren
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
          </Tooltip.Content>
        </Tooltip.Positioner>
      </Portal>
    </Tooltip.Root>
  )
}
