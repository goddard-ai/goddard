import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { ArrowUpRight, CircleDot, FolderGit2, MessageSquareText } from "lucide-react"
import type { DaemonSession } from "@goddard-ai/sdk"
import { getSessionDisplayTitle, getSessionPreviewText } from "./presentation.ts"

function formatTimeLabel(value: number) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value))
}

export function ListRow(props: {
  isSelected: boolean
  onOpen: () => void
  onSelect: () => void
  session: DaemonSession
}) {
  return (
    <article
      class={cx(
        css({
          display: "grid",
          gridTemplateColumns: "minmax(0, 1fr) auto",
          gap: "16px",
          padding: "18px",
          borderRadius: "22px",
          border: "1px solid",
          borderColor: "border",
          background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
          boxShadow: "0 14px 30px rgba(121, 138, 160, 0.08)",
          transition:
            "transform 180ms cubic-bezier(0.23, 1, 0.32, 1), border-color 180ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
          "&[data-selected='true']": {
            borderColor: "accentStrong",
            boxShadow: `0 18px 34px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
          },
          "@media (hover: hover) and (pointer: fine)": {
            _hover: {
              transform: "translateY(-1px)",
              borderColor: "accent",
            },
          },
        }),
      )}
      data-selected={props.isSelected}
    >
      <button
        class={css({
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          gap: "12px",
          minWidth: "0",
          border: "none",
          background: "transparent",
          color: "inherit",
          textAlign: "left",
          cursor: "pointer",
        })}
        type="button"
        onClick={props.onSelect}
      >
        <div
          class={css({
            display: "flex",
            alignItems: "center",
            gap: "10px",
            flexWrap: "wrap",
          })}
        >
          <span
            class={css({
              display: "inline-flex",
              alignItems: "center",
              gap: "8px",
              padding: "6px 10px",
              borderRadius: "999px",
              backgroundColor: "surface",
              color: "accentStrong",
              fontSize: "0.72rem",
              fontWeight: "700",
              letterSpacing: "0.08em",
              textTransform: "uppercase",
            })}
          >
            <CircleDot size={13} strokeWidth={2.1} />
            {props.session.status}
          </span>
          <span
            class={css({
              color: "muted",
              fontSize: "0.77rem",
              fontWeight: "650",
              letterSpacing: "0.08em",
              textTransform: "uppercase",
            })}
          >
            {formatTimeLabel(props.session.updatedAt)}
          </span>
        </div>
        <div class={css({ display: "grid", gap: "8px", minWidth: "0" })}>
          <h2
            class={css({
              color: "text",
              fontSize: "1rem",
              fontWeight: "700",
              lineHeight: "1.3",
            })}
          >
            {getSessionDisplayTitle(props.session)}
          </h2>
          <p
            class={css({
              color: "muted",
              fontSize: "0.9rem",
              lineHeight: "1.6",
              overflow: "hidden",
              lineClamp: "2",
            })}
          >
            {getSessionPreviewText(props.session) || "No transcript yet."}
          </p>
        </div>
        <div
          class={css({
            display: "flex",
            alignItems: "center",
            gap: "14px",
            flexWrap: "wrap",
            color: "muted",
            fontSize: "0.8rem",
          })}
        >
          <span class={css({ display: "inline-flex", alignItems: "center", gap: "6px" })}>
            <MessageSquareText size={14} strokeWidth={2} />
            {props.session.lastAgentMessage ? "Transcript ready" : "Waiting for first prompt"}
          </span>
          <span class={css({ display: "inline-flex", alignItems: "center", gap: "6px" })}>
            <FolderGit2 size={14} strokeWidth={2} />
            {props.session.cwd}
          </span>
        </div>
      </button>
      <button
        class={css({
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          gap: "8px",
          alignSelf: "flex-start",
          height: "38px",
          paddingInline: "12px",
          borderRadius: "12px",
          border: "1px solid",
          borderColor: "border",
          backgroundColor: "background",
          color: "text",
          fontSize: "0.84rem",
          fontWeight: "640",
          cursor: "pointer",
          transition:
            "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
          "@media (hover: hover) and (pointer: fine)": {
            _hover: {
              borderColor: "accent",
              backgroundColor: "surface",
            },
          },
        })}
        type="button"
        onClick={props.onOpen}
      >
        Open
        <ArrowUpRight size={15} strokeWidth={2.2} />
      </button>
    </article>
  )
}
