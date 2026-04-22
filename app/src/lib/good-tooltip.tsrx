/** Shared, pre-styled tooltip primitives for app UI chrome. */
import { Portal } from "@ark-ui/react/portal"
import { Tooltip } from "@ark-ui/react/tooltip"

import styles from "./good-tooltip.style.ts"

/** Attaches the shared app tooltip treatment to one trigger element. */
export function GoodTooltip(props: {
  ariaLabel?: string;
  children: preact.ComponentChildren;
  content: preact.ComponentChildren;
  side?: "top" | "right" | "bottom" | "left";
  sideOffset?: number;
}) {
  return <tsx:react>
    <Tooltip.Root closeDelay={80} closeOnPointerDown={false} openDelay={450} positioning={{
      gutter: props.sideOffset ?? 8,
      placement: props.side ?? "top",
    }}>
      <Tooltip.Trigger asChild>
        {props.children}
      </Tooltip.Trigger>
      <Portal>
        <Tooltip.Positioner>
          <Tooltip.Content aria-label={props.ariaLabel} class={styles.content}>
            {props.content}
          </Tooltip.Content>
        </Tooltip.Positioner>
      </Portal>
    </Tooltip.Root>
  </tsx:react>
}
