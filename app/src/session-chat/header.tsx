import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Clock3, FolderGit2, Sparkles } from "lucide-react"
import type { DaemonSession } from "@goddard-ai/sdk"
import { getSessionDisplayTitle } from "~/sessions/presentation.ts"

function formatTimestamp(value: number) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value))
}

export function Header(props: { messageCount: number; session: DaemonSession }) {
  return (
    <header
      class={css({
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "16px",
        padding: "18px 20px",
        borderBottom: "1px solid",
        borderColor: "border",
        background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
      })}
    >
      <div class={css({ display: "grid", gap: "10px", minWidth: "0" })}>
        <div
          class={css({
            display: "inline-flex",
            alignItems: "center",
            gap: "8px",
            width: "fit-content",
            padding: "7px 11px",
            borderRadius: "999px",
            backgroundColor: "surface",
            color: "accentStrong",
            fontSize: "0.72rem",
            fontWeight: "700",
            letterSpacing: "0.12em",
            textTransform: "uppercase",
          })}
        >
          <Sparkles size={14} strokeWidth={2} />
          Session chat
        </div>
        <div class={css({ display: "grid", gap: "6px" })}>
          <h1
            class={css({
              color: "text",
              fontSize: "1.15rem",
              fontWeight: "740",
              letterSpacing: "-0.03em",
              lineHeight: "1.2",
            })}
          >
            {getSessionDisplayTitle(props.session)}
          </h1>
          <p
            class={css({
              color: "muted",
              fontSize: "0.9rem",
              lineHeight: "1.6",
            })}
          >
            {props.session.lastAgentMessage ?? props.session.cwd}
          </p>
        </div>
      </div>
      <div
        class={css({
          display: "grid",
          gap: "10px",
          justifyItems: "end",
          color: "muted",
          fontSize: "0.8rem",
        })}
      >
        <span class={css({ display: "inline-flex", alignItems: "center", gap: "6px" })}>
          <FolderGit2 size={14} strokeWidth={2} />
          {props.session.cwd}
        </span>
        <span class={css({ display: "inline-flex", alignItems: "center", gap: "6px" })}>
          <Clock3 size={14} strokeWidth={2} />
          {props.messageCount} messages · {formatTimestamp(props.session.updatedAt)}
        </span>
      </div>
    </header>
  )
}
