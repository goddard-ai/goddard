/** Placeholder settings surface rendered inside one closable workbench tab. */
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

import { GoodIcon } from "~/lib/good-icon.tsx"

const pageClass = css({
  display: "flex",
  flexDirection: "column",
  gap: "18px",
  height: "100%",
  padding: "28px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 32%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

const cardClass = css({
  maxWidth: "880px",
  padding: "32px",
  borderRadius: "28px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  boxShadow: "0 28px 80px rgba(121, 138, 160, 0.14)",
})

const badgeClass = css({
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  padding: "8px 12px",
  borderRadius: "999px",
  backgroundColor: "surface",
  marginBottom: "10px",
  color: "accentStrong",
  fontSize: "0.72rem",
  fontWeight: "700",
  letterSpacing: "0.14em",
  textTransform: "uppercase",
})

const titleClass = css({
  marginBottom: "12px",
  color: "text",
  fontSize: "1.4rem",
  fontWeight: "700",
})

const bodyClass = css({
  maxWidth: "64ch",
  color: "muted",
  lineHeight: "1.72",
})

/** Renders the temporary settings placeholder until the real preferences surface exists. */
export function SettingsPage() {
  return (
    <div class={pageClass}>
      <section class={cardClass}>
        <div class={badgeClass}>
          <span class={css({ width: "14px", height: "14px" })}>
            <GoodIcon name="settings" height="14px" width="14px" />
          </span>
          Placeholder
        </div>
        <h1 class={titleClass}>Settings</h1>
        <p class={bodyClass}>
          This tab is a placeholder for the future settings and preferences surface.
        </p>
        <p class={bodyClass}>
          The shell settings button now opens a normal detail tab so the real implementation can
          land without changing the tab flow again.
        </p>
      </section>
    </div>
  )
}

export default SettingsPage
