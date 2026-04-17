import { cx } from "@goddard-ai/styled-system/css"

import { useShortcutRegistry } from "~/app-state-context.tsx"
import { appCommandList } from "~/commands/app-command.ts"
import styles from "./view.style.ts"

/** Renders the detail-tab browser for shortcut-bindable commands and their active bindings. */
export default function KeyboardShortcutsView(props: { class?: string }) {
  const shortcutRegistry = useShortcutRegistry()
  const commandRows = appCommandList.map((command) => ({
    command,
    bindings: shortcutRegistry.resolvedBindings[command.id] ?? [null],
  }))

  return (
    <section class={cx(styles.page, props.class)}>
      <header class={styles.header}>
        <span class={styles.eyebrow}>Workbench Tab</span>
        <div class={styles.intro}>
          <h1 class={styles.title}>Keyboard Shortcuts</h1>
          <p class={styles.description}>
            Browse every shortcut-bindable command, inspect the active binding source, and check
            which bindings are currently gated by a `when` clause. Search, recording search, and
            rebinding land in the next slices.
          </p>
        </div>
        <div class={styles.metaRow}>
          <span class={styles.metaBadge}>
            Profile
            <strong>{shortcutRegistry.selectedProfileId}</strong>
          </span>
          <span class={styles.metaBadge}>
            Commands
            <strong>{commandRows.length}</strong>
          </span>
          <span class={styles.metaBadge}>
            Status
            <strong>
              {shortcutRegistry.isHydrated ? "Overrides loaded" : "Loading overrides"}
            </strong>
          </span>
        </div>
      </header>
      {shortcutRegistry.loadError ? (
        <div class={styles.warning}>
          The saved user keymap could not be loaded, so the built-in profile is active.
          <br />
          {shortcutRegistry.loadError}
        </div>
      ) : null}
      <div class={styles.list}>
        <div class={styles.listHeader}>
          <span>Command</span>
          <span>Shortcut</span>
          <span>When</span>
        </div>
        {commandRows.flatMap((row) => {
          return row.bindings.map((binding) => {
            const bindingId = binding
              ? typeof binding === "string"
                ? binding
                : (binding.combo ?? binding.sequence)
              : null

            return (
              <article
                key={bindingId ? `${row.command.id}:${bindingId}` : row.command.id}
                class={styles.row}
              >
                <div class={styles.labelColumn}>
                  <div class={styles.label}>{row.command.label}</div>
                  <div class={styles.mono}>{row.command.id}</div>
                </div>
                <div class={styles.mono}>{bindingId ?? "–"}</div>
                <div class={styles.mono}>
                  {(binding && typeof binding !== "string" ? binding.when : undefined) ??
                    row.command.when ??
                    "–"}
                </div>
              </article>
            )
          })
        })}
      </div>
    </section>
  )
}
