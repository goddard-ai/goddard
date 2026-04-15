import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"
import { ArrowUpRight } from "lucide-react"

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
      class={css({
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) auto",
        gap: "12px",
        padding: "14px 16px",
        borderInlineStart: "2px solid",
        borderInlineStartColor: props.isSelected ? "accent" : "transparent",
        backgroundColor: props.isSelected ? "surface" : "transparent",
      })}
    >
      <button
        class={css({
          display: "grid",
          gap: "8px",
          minWidth: "0",
          padding: "0",
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
            flexWrap: "wrap",
            alignItems: "center",
            gap: "8px",
          })}
        >
          <span
            class={css({
              color: "text",
              fontSize: "0.78rem",
              fontWeight: "600",
              letterSpacing: "0.04em",
              textTransform: "uppercase",
            })}
          >
            {props.session.status}
          </span>
          <span
            class={css({
              color: "muted",
              fontSize: "0.8rem",
            })}
          >
            Updated {formatTimeLabel(props.session.updatedAt)}
          </span>
        </div>
        <div class={css({ display: "grid", gap: "8px", minWidth: "0" })}>
          <h2
            class={css({
              color: "text",
              fontSize: "0.96rem",
              fontWeight: "600",
              lineHeight: "1.4",
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
            color: "muted",
            fontSize: "0.8rem",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          })}
        >
          {props.session.lastAgentMessage ? "Transcript ready" : "Waiting for first prompt"}
          {" · "}
          {props.session.cwd}
        </div>
      </button>
      <button
        class={css({
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          gap: "8px",
          alignSelf: "flex-start",
          height: "34px",
          paddingInline: "12px",
          borderRadius: "10px",
          border: "1px solid",
          borderColor: "border",
          backgroundColor: "background",
          color: "text",
          fontSize: "0.84rem",
          fontWeight: "600",
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
