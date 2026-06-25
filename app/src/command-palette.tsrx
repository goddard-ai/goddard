import { t } from "@lingui/core/macro"
import type { Signal } from "@preact/signals"

import { useShortcutRegistry } from "./app-state-context.tsrx"
import {
  CommandPaletteSurface,
  type CommandPaletteSurfaceItem,
} from "./command-palette-surface.tsrx"
import styles from "./command-palette.style.ts"
import { appCommandList } from "./commands/app-command.ts"
import { isCommandPaletteVisible } from "./commands/command-context.ts"
import { GoodKeyboardShortcut } from "./lib/good-keyboard-shortcut.tsrx"
import { getShortcutBindingExpression } from "./shared/shortcut-keymap.ts"

export default function CommandPalette(props: { open: Signal<boolean> }) @{
  const shortcutRegistry = useShortcutRegistry()
  const visibleCommands = appCommandList.filter(
    (command) =>
      command.id !== "navigation.openCommandPalette" &&
        isCommandPaletteVisible(shortcutRegistry.runtime, command),
  )
  const items = visibleCommands.map((command): CommandPaletteSurfaceItem => {
    const shortcut = getShortcutBindingExpression(
      shortcutRegistry.resolvedBindings[command.id]?.[0],
    )

    return {
      id: command.id,
      icon: command.icon,
      keywords: command.keywords,
      label: command.label,
      suffix: shortcut
        ? <GoodKeyboardShortcut class={styles.shortcut} expression={shortcut} />
        : null,
    }
  })

  const selectCommand = (item: CommandPaletteSurfaceItem) => {
    const command = visibleCommands.find((command) => command.id === item.id)

    if (!command) {
      return
    }

    props.open.value = false
    window.setTimeout(() => {
      command()
    }, 0)
  }

  <CommandPaletteSurface
    emptyText={t`No matching commands.`}
    {items}
    listLabel={t`Command menu`}
    open={props.open}
    placeholder={t`Type a command`}
    title={t`Command menu`}
    onSelect={selectCommand}
  />
}
