import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import type { ITheme } from "@xterm/headless"
import { useEffect, useRef } from "preact/hooks"
import { useSigma } from "preact-sigma"
import {
  TerminalViewportModel,
  translateKeyboardEvent,
  type TerminalViewportChunk,
  type TerminalViewportFont,
} from "./state/TerminalViewportModel"

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
  sessionId: string
  chunks?: readonly TerminalViewportChunk[]
  theme?: ITheme
  font?: TerminalViewportFont
  minimumCols?: number
  minimumRows?: number
  onReady?: () => void
  onResize?: (cols: number, rows: number) => void
  onInput?: (data: string) => void
  onPaste?: (data: string) => void
  onFocus?: () => void
  class?: string
  ariaLabel?: string
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const terminalViewport = useSigma(() => new TerminalViewportModel())
  const themeKey = JSON.stringify(props.theme ?? {})
  const fontKey = JSON.stringify(props.font ?? {})

  useEffect(() => {
    terminalViewport.attachViewport(viewportRef.current)
    return () => {
      terminalViewport.attachViewport(null)
    }
  }, [terminalViewport])

  useEffect(() => {
    terminalViewport.configureViewport({
      sessionId: props.sessionId,
      theme: props.theme,
      font: props.font,
      minimumCols: props.minimumCols,
      minimumRows: props.minimumRows,
      onReady: props.onReady,
      onResize: props.onResize,
      onInput: props.onInput,
      onPaste: props.onPaste,
      onFocus: props.onFocus,
    })
  }, [
    fontKey,
    props.minimumCols,
    props.minimumRows,
    props.onFocus,
    props.onInput,
    props.onPaste,
    props.onReady,
    props.onResize,
    props.sessionId,
    terminalViewport,
    themeKey,
  ])

  useEffect(() => {
    terminalViewport.syncChunks(props.chunks ?? [])
  }, [props.chunks, terminalViewport])

  return (
    <div class={cx(frameClass, props.class)}>
      <div
        aria-label={props.ariaLabel ?? "Terminal viewport"}
        class={viewportClass}
        onFocus={() => {
          terminalViewport.notifyFocus()
        }}
        onKeyDown={(event) => {
          const input = translateKeyboardEvent(event)

          if (!input) {
            return
          }

          event.preventDefault()
          terminalViewport.forwardInput(input)
        }}
        onPaste={(event) => {
          const pasted = event.clipboardData?.getData("text")

          if (!pasted) {
            return
          }

          event.preventDefault()
          terminalViewport.forwardPaste(pasted)
        }}
        onWheel={(event) => {
          event.preventDefault()
          terminalViewport.scrollViewport(event.deltaY, event.deltaMode)
        }}
        ref={viewportRef}
        role="region"
        style={{
          backgroundColor: terminalViewport.theme.background,
          color: terminalViewport.theme.foreground,
          fontFamily: terminalViewport.fontFamily,
          fontSize: `${terminalViewport.fontSize}px`,
          fontVariantLigatures: "none",
          letterSpacing: `${terminalViewport.letterSpacing}px`,
          lineHeight: String(terminalViewport.lineHeight),
        }}
        tabIndex={0}
      >
        {terminalViewport.viewRows.map((row) => (
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
