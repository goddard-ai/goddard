import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Plus, Rows3 } from "lucide-react"

export function ListToolbar(props: { sessionCount: number; onCreateSession: () => void }) {
  return (
    <header
      class={css({
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "16px",
        padding: "22px 24px 18px",
        borderBottom: "1px solid",
        borderColor: "border",
      })}
    >
      <div
        class={css({
          display: "flex",
          flexDirection: "column",
          gap: "10px",
          minWidth: "0",
        })}
      >
        <span
          class={css({
            display: "inline-flex",
            alignItems: "center",
            gap: "8px",
            width: "fit-content",
            padding: "8px 12px",
            borderRadius: "999px",
            backgroundColor: "surface",
            color: "accentStrong",
            fontSize: "0.72rem",
            fontWeight: "700",
            letterSpacing: "0.14em",
            textTransform: "uppercase",
          })}
        >
          <Rows3 size={14} strokeWidth={2} />
          Session index
        </span>
        <div class={css({ display: "flex", flexDirection: "column", gap: "6px" })}>
          <h1
            class={css({
              color: "text",
              fontSize: "1.45rem",
              fontWeight: "760",
              letterSpacing: "-0.03em",
              lineHeight: "1.1",
            })}
          >
            Sessions
          </h1>
          <p
            class={css({
              color: "muted",
              fontSize: "0.93rem",
              lineHeight: "1.6",
            })}
          >
            {props.sessionCount === 0
              ? "Launch the first local session to seed the chat flow."
              : `${props.sessionCount} session${props.sessionCount === 1 ? "" : "s"} available in this bootstrap shell.`}
          </p>
        </div>
      </div>
      <button
        class={cx(
          css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            gap: "8px",
            height: "40px",
            paddingInline: "14px",
            borderRadius: "14px",
            border: "1px solid",
            borderColor: "accent",
            background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
            color: "text",
            fontSize: "0.88rem",
            fontWeight: "640",
            cursor: "pointer",
            boxShadow: `0 12px 28px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent)`,
            transition:
              "transform 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
            _active: {
              transform: "scale(0.97)",
            },
            _focusVisible: {
              outline: `2px solid ${token.var("colors.accentStrong")}`,
              outlineOffset: "2px",
            },
            "@media (hover: hover) and (pointer: fine)": {
              _hover: {
                borderColor: "accentStrong",
                boxShadow: `0 18px 34px color-mix(in srgb, ${token.var("colors.accent")} 18%, transparent)`,
              },
            },
          }),
        )}
        type="button"
        onClick={props.onCreateSession}
      >
        <Plus size={16} strokeWidth={2.2} />
        New session
      </button>
    </header>
  )
}
