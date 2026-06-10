import { useShortcutRegistry } from "~/app-state-context.tsrx"
import { GoodKeyboardShortcut } from "~/lib/good-keyboard-shortcut.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { getShortcutBindingExpression } from "~/shared/shortcut-keymap.ts"
import type { AppCommand as AppCommandType } from "./app-command.ts"
import styles from "./app-command-tooltip.style.ts"

export function AppCommandTooltip(props: {
  children: preact.ComponentChildren;
  command: AppCommandType;
  group?: string;
  label?: string;
  side?: "top" | "right" | "bottom" | "left";
  sideOffset?: number;
}) @{
  const shortcutRegistry = useShortcutRegistry()
  const shortcut = getShortcutBindingExpression(
    shortcutRegistry.resolvedBindings[props.command.id]?.[0],
  )
  const label = props.label ?? props.command.label

  <GoodTooltip
    ariaLabel={label}
    content={@{
      <span class={styles.content}>
        <span>{label}</span>

        @if (shortcut) {
          <GoodKeyboardShortcut class={styles.shortcut} expression={shortcut} />
        }
      </span>
    }}
    group={props.group}
    side={props.side}
    sideOffset={props.sideOffset}
  >{props.children}</GoodTooltip>
}
