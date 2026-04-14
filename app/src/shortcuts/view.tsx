import { css, cx } from "@goddard-ai/styled-system/css"

import { useShortcutRegistry } from "~/app-state-context.tsx"
import { appCommandList } from "~/commands/app-command.ts"

const pageClass = css({
  display: "grid",
  gap: "20px",
  minHeight: "100%",
  padding: "28px",
  background: "linear-gradient(180deg, rgba(18, 21, 25, 0.96) 0%, rgba(14, 16, 19, 0.98) 100%)",
})

const eyebrowClass = css({
  display: "inline-flex",
  alignItems: "center",
  width: "fit-content",
  padding: "6px 10px",
  borderRadius: "999px",
  backgroundColor: "rgba(86, 160, 255, 0.14)",
  color: "#8fc1ff",
  fontSize: "11px",
  fontWeight: "700",
  letterSpacing: "0.12em",
  textTransform: "uppercase",
})

const titleClass = css({
  color: "#f5f7fa",
  fontSize: "28px",
  fontWeight: "720",
  letterSpacing: "-0.035em",
  lineHeight: "1.05",
})

const descriptionClass = css({
  maxWidth: "680px",
  color: "rgba(218, 225, 232, 0.72)",
  fontSize: "14px",
  lineHeight: "1.75",
})

const metaRowClass = css({
  display: "flex",
  flexWrap: "wrap",
  gap: "12px",
  alignItems: "center",
})

const metaBadgeClass = css({
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  padding: "7px 11px",
  borderRadius: "999px",
  backgroundColor: "rgba(255, 255, 255, 0.05)",
  color: "rgba(232, 237, 241, 0.82)",
  fontSize: "12px",
  letterSpacing: "0.03em",
})

const warningClass = css({
  padding: "14px 16px",
  borderRadius: "16px",
  border: "1px solid rgba(255, 166, 102, 0.22)",
  backgroundColor: "rgba(255, 166, 102, 0.08)",
  color: "#ffd0ab",
  fontSize: "13px",
  lineHeight: "1.6",
})

const listClass = css({
  display: "grid",
  gap: "10px",
})

const listHeaderClass = css({
  display: "grid",
  gridTemplateColumns: "minmax(0, 2.2fr) minmax(160px, 1fr) minmax(160px, 1fr)",
  gap: "16px",
  paddingInline: "16px",
  color: "rgba(201, 209, 217, 0.48)",
  fontSize: "11px",
  fontWeight: "700",
  letterSpacing: "0.12em",
  textTransform: "uppercase",
})

const rowClass = css({
  display: "grid",
  gridTemplateColumns: "minmax(0, 2.2fr) minmax(160px, 1fr) minmax(160px, 1fr)",
  gap: "16px",
  alignItems: "start",
  padding: "16px",
  borderRadius: "18px",
  border: "1px solid rgba(255, 255, 255, 0.06)",
  backgroundColor: "rgba(255, 255, 255, 0.03)",
})

const labelColumnClass = css({
  display: "grid",
  gap: "6px",
  minWidth: "0",
})

const labelClass = css({
  color: "#f7f9fb",
  fontSize: "15px",
  fontWeight: "630",
  letterSpacing: "-0.02em",
})

const monoValueClass = css({
  fontFamily: '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace',
  fontSize: "12px",
  lineHeight: "1.7",
  color: "rgba(225, 232, 239, 0.82)",
  whiteSpace: "pre-wrap",
})

/** Renders the detail-tab browser for shortcut-bindable commands and their active bindings. */
export default function KeyboardShortcutsView(props: { class?: string }) {
  const shortcutRegistry = useShortcutRegistry()
  const commandRows = appCommandList.map((command) => ({
    command,
    bindings: shortcutRegistry.resolvedBindings[command.id] ?? [null],
  }))

  return (
    <section class={cx(pageClass, props.class)}>
      <header class={css({ display: "grid", gap: "14px" })}>
        <span class={eyebrowClass}>Workbench Tab</span>
        <div class={css({ display: "grid", gap: "10px" })}>
          <h1 class={titleClass}>Keyboard Shortcuts</h1>
          <p class={descriptionClass}>
            Browse every shortcut-bindable command, inspect the active binding source, and check
            which bindings are currently gated by a `when` clause. Search, recording search, and
            rebinding land in the next slices.
          </p>
        </div>
        <div class={metaRowClass}>
          <span class={metaBadgeClass}>
            Profile
            <strong>{shortcutRegistry.selectedProfileId}</strong>
          </span>
          <span class={metaBadgeClass}>
            Commands
            <strong>{commandRows.length}</strong>
          </span>
          <span class={metaBadgeClass}>
            Status
            <strong>
              {shortcutRegistry.isHydrated ? "Overrides loaded" : "Loading overrides"}
            </strong>
          </span>
        </div>
      </header>
      {shortcutRegistry.loadError ? (
        <div class={warningClass}>
          The saved user keymap could not be loaded, so the built-in profile is active.
          <br />
          {shortcutRegistry.loadError}
        </div>
      ) : null}
      <div class={listClass}>
        <div class={listHeaderClass}>
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
                class={rowClass}
              >
                <div class={labelColumnClass}>
                  <div class={labelClass}>{row.command.label}</div>
                  <div class={monoValueClass}>{row.command.id}</div>
                </div>
                <div class={monoValueClass}>{bindingId ?? "–"}</div>
                <div class={monoValueClass}>
                  {binding && typeof binding !== "string" ? binding.when : "–"}
                </div>
              </article>
            )
          })
        })}
      </div>
    </section>
  )
}
