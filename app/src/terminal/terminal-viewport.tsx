import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useEffect, useRef } from "preact/hooks"
import { translateKeyboardEvent, type TerminalViewportModel } from "./terminal-viewport-model"

const VIEWPORT_PADDING_PX = 18

const frameClass = css({
  display: "flex",
  minHeight: "280px",
  minWidth: "0",
  borderRadius: "24px",
  border: "1px solid",
  borderColor: "border",
  overflow: "hidden",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  boxShadow: "0 24px 64px rgba(15, 23, 42, 0.14)",
})

const viewportClass = css({
  flex: "1 1 auto",
  minWidth: "0",
  minHeight: "0",
  overflow: "auto",
  padding: `${VIEWPORT_PADDING_PX}px`,
  outline: "none",
  whiteSpace: "pre",
  userSelect: "text",
  cursor: "text",
  scrollbarWidth: "thin",
  scrollbarColor: `${token.var("colors.accentStrong")} transparent`,
  _focusVisible: {
    boxShadow: `inset 0 0 0 2px color-mix(in srgb, ${token.var("colors.accent")} 26%, transparent)`,
  },
})

const rowClass = css({
  display: "block",
  minWidth: "fit-content",
})

export function TerminalViewport(props: {
  terminal: TerminalViewportModel
  class?: string
  ariaLabel?: string
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    props.terminal.attachViewport(viewportRef.current)
    return () => {
      props.terminal.attachViewport(null)
    }
  }, [props.terminal])

  return (
    <div class={cx(frameClass, props.class)}>
      <div
        aria-label={props.ariaLabel ?? "Terminal viewport"}
        class={viewportClass}
        onFocus={() => {
          props.terminal.notifyFocus()
        }}
        onKeyDown={(event) => {
          const input = translateKeyboardEvent(event)

          if (!input) {
            return
          }

          event.preventDefault()
          props.terminal.forwardInput(input)
        }}
        onPaste={(event) => {
          const pasted = event.clipboardData?.getData("text")

          if (!pasted) {
            return
          }

          event.preventDefault()
          props.terminal.forwardPaste(pasted)
        }}
        onWheel={(event) => {
          event.preventDefault()
          props.terminal.scrollViewport(event.deltaY, event.deltaMode)
        }}
        ref={viewportRef}
        role="region"
        style={{
          backgroundColor: props.terminal.theme.background,
          color: props.terminal.theme.foreground,
          fontFamily: props.terminal.fontFamily,
          fontSize: props.terminal.fontSize,
          fontVariantLigatures: "none",
          letterSpacing: props.terminal.letterSpacing,
          lineHeight: props.terminal.lineHeight,
        }}
        tabIndex={0}
      >
        {props.terminal.viewRows.map((row) => (
          <div class={rowClass} key={row.key}>
            {row.segments.length === 0 ? " " : null}
            {row.segments.map((segment) => (
              <span key={segment.key} style={segment.style}>
                {segment.text}
              </span>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
