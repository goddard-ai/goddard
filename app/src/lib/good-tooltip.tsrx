/** Shared, pre-styled tooltip primitives for app UI chrome. */
import styles from "./good-tooltip.style.ts"
import { Tooltip } from "./tooltip.tsx"

/** Attaches the shared app tooltip treatment to one trigger element. */
export component GoodTooltip(props: {
  ariaLabel?: string;
  children: preact.ComponentChildren;
  content: preact.ComponentChildren;
  side?: "top" | "right" | "bottom" | "left";
  sideOffset?: number;
}) {
  <Tooltip
    ariaLabel={props.ariaLabel}
    class={styles.content}
    closeDelay={80}
    content={props.content}
    openDelay={450}
    side={props.side}
    sideOffset={props.sideOffset}
  >
    {props.children}
  </Tooltip>
}
