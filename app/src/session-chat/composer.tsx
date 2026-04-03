import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { SendHorizontal } from "lucide-react"

export function Composer(props: { onSubmit: (text: string) => void }) {
  const draft = useSignal("")
  const canSubmit = draft.value.trim().length > 0

  function submit() {
    const trimmedDraft = draft.value.trim()

    if (trimmedDraft.length === 0) {
      return
    }

    props.onSubmit(trimmedDraft)
    draft.value = ""
  }

  return (
    <form
      class={css({
        display: "grid",
        gap: "14px",
        padding: "16px 18px 18px",
        borderTop: "1px solid",
        borderColor: "border",
        background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
      })}
      onSubmit={(event) => {
        event.preventDefault()
        submit()
      }}
    >
      <textarea
        class={cx(
          css({
            width: "100%",
            minHeight: "96px",
            padding: "14px 16px",
            borderRadius: "18px",
            border: "1px solid",
            borderColor: "border",
            backgroundColor: "background",
            color: "text",
            fontSize: "0.94rem",
            lineHeight: "1.6",
            resize: "vertical",
            outline: "none",
            transition:
              "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
            _focusVisible: {
              borderColor: "accentStrong",
              boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
            },
          }),
        )}
        placeholder="Add the next instruction for this session."
        value={draft.value}
        onInput={(event) => {
          draft.value = event.currentTarget.value
        }}
      />
      <div
        class={css({
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: "12px",
        })}
      >
        <p
          class={css({
            color: "muted",
            fontSize: "0.83rem",
            lineHeight: "1.6",
          })}
        >
          Prompts are sent to the live daemon-backed session and the transcript refreshes from
          session history.
        </p>
        <button
          class={css({
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            gap: "8px",
            minWidth: "112px",
            height: "40px",
            paddingInline: "14px",
            borderRadius: "14px",
            border: "1px solid",
            borderColor: "accent",
            background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
            color: "text",
            fontSize: "0.88rem",
            fontWeight: "680",
            cursor: "pointer",
            _disabled: {
              cursor: "not-allowed",
              opacity: "0.52",
            },
          })}
          disabled={!canSubmit}
          type="submit"
        >
          Send
          <SendHorizontal size={15} strokeWidth={2.2} />
        </button>
      </div>
    </form>
  )
}
