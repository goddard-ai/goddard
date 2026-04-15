import { css, cx } from "@goddard-ai/styled-system/css"
import { Plus } from "lucide-react"

export function ListToolbar(props: { sessionCount: number; onCreateSession: () => void }) {
  return (
    <header
      class={css({
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "space-between",
        gap: "16px",
        padding: "20px",
        borderBottom: "1px solid",
        borderColor: "border",
      })}
    >
      <div class={css({ display: "grid", gap: "6px", minWidth: "0" })}>
        <h1
          class={css({
            color: "text",
            fontSize: "1.25rem",
            fontWeight: "700",
            lineHeight: "1.25",
          })}
        >
          Sessions
        </h1>
        <p
          class={css({
            color: "muted",
            fontSize: "0.9rem",
            lineHeight: "1.6",
          })}
        >
          {props.sessionCount === 0
            ? "Launch the first local session to seed the chat flow."
            : `${props.sessionCount} session${props.sessionCount === 1 ? "" : "s"} available in the workbench.`}
        </p>
      </div>
      <button
        class={cx(
          css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            gap: "8px",
            height: "36px",
            paddingInline: "12px",
            borderRadius: "10px",
            border: "1px solid",
            borderColor: "accent",
            backgroundColor: "surface",
            color: "text",
            fontSize: "0.86rem",
            fontWeight: "600",
            cursor: "pointer",
            transition:
              "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
            "@media (hover: hover) and (pointer: fine)": {
              _hover: {
                borderColor: "accentStrong",
                backgroundColor: "background",
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
