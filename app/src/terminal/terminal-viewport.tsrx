import { cx } from "@goddard-ai/styled-system/css"
import type { Protected } from "preact-sigma"
import { useEffect } from "preact/hooks"

import { translateKeyboardEvent, type TerminalViewportModel } from "./terminal-viewport-model.ts"
import styles from "./terminal-viewport.style.ts"

export component TerminalViewport(props: {
  terminal: Protected<TerminalViewportModel>;
  class?: string;
  ariaLabel?: string;
}) {
  let viewportElement: HTMLDivElement | null = null

  useEffect(() => {
    props.terminal.attachViewport(viewportElement)
    return () => {
      props.terminal.attachViewport(null)
    }
  }, [props.terminal])

  <div class={cx(styles.frame, props.class)}>
    <div
      aria-label={props.ariaLabel ?? "Terminal viewport"}
      class={styles.viewport}
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
      {ref viewportElement}
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
      for (const row of props.terminal.viewRows) {
        <div class={styles.row} key={row.key}>
          {row.segments.length === 0 ? " " : null}
          for (const segment of row.segments) {
            <span key={segment.key} style={segment.style}>
              {segment.text}
            </span>
          }
        </div>
      }
    </div>
  </div>
}
