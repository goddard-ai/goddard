import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { Terminal, type ITheme } from "@xterm/headless"
import type { CSSProperties, TargetedKeyboardEvent } from "preact"
import { useEffect, useRef } from "preact/hooks"

const VIEWPORT_PADDING_PX = 18

const defaultTheme: ITheme = {
  background: "#0d1117",
  foreground: "#e6edf3",
  cursor: "#f4d58d",
  cursorAccent: "#0d1117",
  selection: "rgba(148, 163, 184, 0.28)",
  black: "#151b23",
  red: "#ff7b72",
  green: "#3fb950",
  yellow: "#d29922",
  blue: "#79c0ff",
  magenta: "#bc8cff",
  cyan: "#39c5cf",
  white: "#d2dae3",
  brightBlack: "#6e7681",
  brightRed: "#ffa198",
  brightGreen: "#56d364",
  brightYellow: "#e3b341",
  brightBlue: "#a5d6ff",
  brightMagenta: "#d2a8ff",
  brightCyan: "#56d4dd",
  brightWhite: "#f0f6fc",
}

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

type TerminalViewportChunk = {
  id: string
  data: string | Uint8Array
}

type TerminalViewportFont = {
  family?: string
  size?: number
  lineHeight?: number
  letterSpacing?: number
}

type TerminalViewportSegment = {
  key: string
  style: CSSProperties
  text: string
}

type TerminalViewportRow = {
  key: string
  segments: TerminalViewportSegment[]
}

type TerminalViewportSnapshot = {
  cols: number
  rows: number
  viewRows: TerminalViewportRow[]
}

type TerminalCell = ReturnType<Terminal["buffer"]["active"]["getNullCell"]>

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
  const terminalRef = useRef<Terminal | null>(null)
  const resizeObserverRef = useRef<ResizeObserver | null>(null)
  const writeVersionRef = useRef(0)
  const processedChunkCountRef = useRef(0)
  const onReadyRef = useRef(props.onReady)
  const onResizeRef = useRef(props.onResize)
  const themeRef = useRef(mergeTerminalTheme(props.theme))
  const snapshot = useSignal<TerminalViewportSnapshot>({
    cols: props.minimumCols ?? 80,
    rows: props.minimumRows ?? 24,
    viewRows: [],
  })
  const fontFamily = props.font?.family ?? '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace'
  const fontSize = props.font?.size ?? 13
  const lineHeight = props.font?.lineHeight ?? 1.45
  const letterSpacing = props.font?.letterSpacing ?? 0
  const resolvedTheme = mergeTerminalTheme(props.theme)
  const themeKey = JSON.stringify(props.theme ?? {})

  useEffect(() => {
    onReadyRef.current = props.onReady
  }, [props.onReady])

  useEffect(() => {
    onResizeRef.current = props.onResize
  }, [props.onResize])

  useEffect(() => {
    const terminal = new Terminal({
      cols: Math.max(props.minimumCols ?? 80, 1),
      rows: Math.max(props.minimumRows ?? 24, 1),
      convertEol: true,
      cursorBlink: true,
      lineHeight,
      letterSpacing,
      scrollback: 5000,
      theme: themeRef.current,
    })

    terminalRef.current = terminal

    const refreshSnapshot = () => {
      snapshot.value = buildViewportSnapshot(terminal, themeRef.current)
    }

    const disposables = [
      terminal.onScroll(() => {
        refreshSnapshot()
      }),
      terminal.onWriteParsed(() => {
        refreshSnapshot()
      }),
      terminal.onResize((nextSize) => {
        snapshot.value = buildViewportSnapshot(terminal, themeRef.current)
        onResizeRef.current?.(nextSize.cols, nextSize.rows)
      }),
      terminal.onTitleChange(() => {
        refreshSnapshot()
      }),
    ]

    refreshSnapshot()
    onReadyRef.current?.()

    return () => {
      writeVersionRef.current += 1
      resizeObserverRef.current?.disconnect()
      resizeObserverRef.current = null

      for (const disposable of disposables) {
        disposable.dispose()
      }

      terminal.dispose()
      terminalRef.current = null
    }
  }, [])

  useEffect(() => {
    const terminal = terminalRef.current

    if (!terminal) {
      return
    }

    themeRef.current = mergeTerminalTheme(props.theme)

    terminal.options = {
      ...terminal.options,
      lineHeight,
      letterSpacing,
      theme: themeRef.current,
    }

    snapshot.value = buildViewportSnapshot(terminal, themeRef.current)
  }, [letterSpacing, lineHeight, themeKey])

  useEffect(() => {
    const terminal = terminalRef.current

    if (!terminal) {
      return
    }

    processedChunkCountRef.current = 0
    writeVersionRef.current += 1
    terminal.reset()
    terminal.clear()
    snapshot.value = buildViewportSnapshot(terminal, themeRef.current)
  }, [props.sessionId])

  useEffect(() => {
    const terminal = terminalRef.current

    if (!terminal) {
      return
    }

    const chunks = props.chunks ?? []

    if (chunks.length < processedChunkCountRef.current) {
      processedChunkCountRef.current = 0
      writeVersionRef.current += 1
      terminal.reset()
      terminal.clear()
    }

    if (chunks.length === processedChunkCountRef.current) {
      return
    }

    const nextVersion = writeVersionRef.current + 1
    writeVersionRef.current = nextVersion

    void (async () => {
      for (const chunk of chunks.slice(processedChunkCountRef.current)) {
        await writeToTerminal(terminal, chunk.data)

        if (writeVersionRef.current !== nextVersion) {
          return
        }
      }

      processedChunkCountRef.current = chunks.length
      snapshot.value = buildViewportSnapshot(terminal, themeRef.current)
    })()
  }, [props.chunks, props.sessionId])

  useEffect(() => {
    const terminal = terminalRef.current
    const viewport = viewportRef.current

    if (!terminal || !viewport) {
      return
    }

    const fitTerminal = () => {
      const nextSize = measureTerminalSize(
        viewport.clientWidth,
        viewport.clientHeight,
        fontFamily,
        fontSize,
        lineHeight,
        letterSpacing,
        props.minimumCols ?? 40,
        props.minimumRows ?? 8,
      )

      if (nextSize.cols !== terminal.cols || nextSize.rows !== terminal.rows) {
        terminal.resize(nextSize.cols, nextSize.rows)
      }
    }

    fitTerminal()

    const observer = new ResizeObserver(() => {
      fitTerminal()
    })

    observer.observe(viewport)
    resizeObserverRef.current = observer

    return () => {
      observer.disconnect()

      if (resizeObserverRef.current === observer) {
        resizeObserverRef.current = null
      }
    }
  }, [
    fontFamily,
    fontSize,
    letterSpacing,
    lineHeight,
    props.minimumCols,
    props.minimumRows,
    props.sessionId,
  ])

  return (
    <div class={cx(frameClass, props.class)}>
      <div
        aria-label={props.ariaLabel ?? "Terminal viewport"}
        class={viewportClass}
        onFocus={() => {
          props.onFocus?.()
        }}
        onKeyDown={(event) => {
          const input = translateKeyboardEvent(event)

          if (!input) {
            return
          }

          event.preventDefault()
          props.onInput?.(input)
        }}
        onPaste={(event) => {
          const pasted = event.clipboardData?.getData("text")

          if (!pasted) {
            return
          }

          event.preventDefault()
          props.onPaste?.(pasted)
          props.onInput?.(pasted)
        }}
        onWheel={(event) => {
          const terminal = terminalRef.current

          if (!terminal) {
            return
          }

          const lines =
            event.deltaMode === WheelEvent.DOM_DELTA_LINE
              ? Math.trunc(event.deltaY)
              : Math.trunc(event.deltaY / Math.max(fontSize * lineHeight, 1))

          if (lines === 0) {
            return
          }

          event.preventDefault()
          terminal.scrollLines(lines)
          snapshot.value = buildViewportSnapshot(terminal, themeRef.current)
        }}
        ref={viewportRef}
        role="region"
        style={{
          backgroundColor: resolvedTheme.background,
          color: resolvedTheme.foreground,
          fontFamily,
          fontSize: `${fontSize}px`,
          fontVariantLigatures: "none",
          letterSpacing: `${letterSpacing}px`,
          lineHeight: String(lineHeight),
        }}
        tabIndex={0}
      >
        {snapshot.value.viewRows.map((row) => (
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

function buildViewportSnapshot(terminal: Terminal, theme: ITheme): TerminalViewportSnapshot {
  const buffer = terminal.buffer.active
  const cursorLine = buffer.baseY + buffer.cursorY
  const viewRows: TerminalViewportRow[] = []

  for (let index = 0; index < terminal.rows; index += 1) {
    const absoluteLine = buffer.viewportY + index
    const line = buffer.getLine(absoluteLine)
    const segments: TerminalViewportSegment[] = []
    const nullCell = buffer.getNullCell()
    let currentText = ""
    let currentStyle: CSSProperties | null = null
    let currentStyleKey = ""

    for (let column = 0; column < terminal.cols; column += 1) {
      const cell = line && column < line.length ? line.getCell(column, nullCell) : undefined

      if (cell?.getWidth() === 0) {
        continue
      }

      const nextStyle = resolveCellStyle(
        cell,
        theme,
        absoluteLine === cursorLine && column === buffer.cursorX,
      )
      const nextStyleKey = serializeStyle(nextStyle)
      const nextText = cell?.getChars() || " "

      if (currentStyleKey !== nextStyleKey) {
        if (currentText.length > 0 && currentStyle) {
          segments.push({
            key: `${absoluteLine}:${segments.length}`,
            style: currentStyle,
            text: currentText,
          })
        }

        currentStyle = nextStyle
        currentStyleKey = nextStyleKey
        currentText = nextText
      } else {
        currentText += nextText
      }
    }

    if (absoluteLine === cursorLine && buffer.cursorX >= terminal.cols) {
      const cursorStyle = resolveCellStyle(undefined, theme, true)
      const cursorStyleKey = serializeStyle(cursorStyle)

      if (currentStyleKey !== cursorStyleKey) {
        if (currentText.length > 0 && currentStyle) {
          segments.push({
            key: `${absoluteLine}:${segments.length}`,
            style: currentStyle,
            text: currentText,
          })
        }

        currentStyle = cursorStyle
        currentStyleKey = cursorStyleKey
        currentText = " "
      } else {
        currentText += " "
      }
    }

    if (currentText.length > 0 && currentStyle) {
      segments.push({
        key: `${absoluteLine}:${segments.length}`,
        style: currentStyle,
        text: currentText,
      })
    }

    viewRows.push({
      key: `${absoluteLine}:${line?.translateToString(false, 0, terminal.cols) ?? ""}`,
      segments,
    })
  }

  return {
    cols: terminal.cols,
    rows: terminal.rows,
    viewRows,
  }
}

function mergeTerminalTheme(theme?: ITheme): ITheme {
  return {
    ...defaultTheme,
    ...theme,
  }
}

function resolveCellStyle(
  cell: TerminalCell | undefined,
  theme: ITheme,
  isCursor: boolean,
): CSSProperties {
  let foreground = theme.foreground
  let background = undefined as string | undefined

  if (cell) {
    foreground = cell.isFgDefault()
      ? theme.foreground
      : cell.isFgRGB()
        ? toHexColor(cell.getFgColor())
        : cell.isFgPalette()
          ? resolvePaletteColor(cell.getFgColor(), theme)
          : theme.foreground

    background = cell.isBgDefault()
      ? undefined
      : cell.isBgRGB()
        ? toHexColor(cell.getBgColor())
        : cell.isBgPalette()
          ? resolvePaletteColor(cell.getBgColor(), theme)
          : undefined

    if (cell.isInverse()) {
      const swappedForeground = background ?? theme.background
      background = foreground ?? theme.foreground
      foreground = swappedForeground
    }
  }

  if (isCursor) {
    return {
      backgroundColor: theme.cursor ?? theme.foreground,
      color: theme.cursorAccent ?? theme.background,
      fontWeight: "700",
    }
  }

  const textDecoration = [
    cell?.isUnderline() ? "underline" : "",
    cell?.isStrikethrough() ? "line-through" : "",
    cell?.isOverline() ? "overline" : "",
  ]
    .filter(Boolean)
    .join(" ")

  return {
    backgroundColor: background,
    color: cell?.isInvisible() ? "transparent" : foreground,
    fontStyle: cell?.isItalic() ? "italic" : undefined,
    fontWeight: cell?.isBold() ? "700" : undefined,
    opacity: cell?.isDim() ? "0.72" : undefined,
    textDecoration: textDecoration.length > 0 ? textDecoration : undefined,
  }
}

function resolvePaletteColor(colorIndex: number, theme: ITheme): string {
  const palette = [
    theme.black ?? defaultTheme.black!,
    theme.red ?? defaultTheme.red!,
    theme.green ?? defaultTheme.green!,
    theme.yellow ?? defaultTheme.yellow!,
    theme.blue ?? defaultTheme.blue!,
    theme.magenta ?? defaultTheme.magenta!,
    theme.cyan ?? defaultTheme.cyan!,
    theme.white ?? defaultTheme.white!,
    theme.brightBlack ?? defaultTheme.brightBlack!,
    theme.brightRed ?? defaultTheme.brightRed!,
    theme.brightGreen ?? defaultTheme.brightGreen!,
    theme.brightYellow ?? defaultTheme.brightYellow!,
    theme.brightBlue ?? defaultTheme.brightBlue!,
    theme.brightMagenta ?? defaultTheme.brightMagenta!,
    theme.brightCyan ?? defaultTheme.brightCyan!,
    theme.brightWhite ?? defaultTheme.brightWhite!,
  ]

  if (colorIndex < palette.length) {
    return palette[colorIndex]
  }

  const extended = theme.extendedAnsi?.[colorIndex - palette.length]

  if (extended) {
    return extended
  }

  if (colorIndex >= 16 && colorIndex <= 231) {
    const cubeIndex = colorIndex - 16
    const red = Math.floor(cubeIndex / 36)
    const green = Math.floor((cubeIndex % 36) / 6)
    const blue = cubeIndex % 6
    const steps = [0, 95, 135, 175, 215, 255]

    return rgbToHex(steps[red] ?? 0, steps[green] ?? 0, steps[blue] ?? 0)
  }

  if (colorIndex >= 232 && colorIndex <= 255) {
    const grayscale = 8 + (colorIndex - 232) * 10
    return rgbToHex(grayscale, grayscale, grayscale)
  }

  return theme.foreground ?? defaultTheme.foreground!
}

function serializeStyle(style: CSSProperties): string {
  return JSON.stringify(style)
}

function toHexColor(color: number): string {
  return `#${color.toString(16).padStart(6, "0")}`
}

function rgbToHex(red: number, green: number, blue: number): string {
  return toHexColor((red << 16) | (green << 8) | blue)
}

function measureTerminalSize(
  viewportWidth: number,
  viewportHeight: number,
  fontFamily: string,
  fontSize: number,
  lineHeight: number,
  letterSpacing: number,
  minimumCols: number,
  minimumRows: number,
): { cols: number; rows: number } {
  const canvas = document.createElement("canvas")
  const context = canvas.getContext("2d")
  const fallbackWidth = fontSize * 0.62 + letterSpacing

  if (!context) {
    return {
      cols: Math.max(
        minimumCols,
        Math.floor((viewportWidth - VIEWPORT_PADDING_PX * 2) / fallbackWidth),
      ),
      rows: Math.max(
        minimumRows,
        Math.floor((viewportHeight - VIEWPORT_PADDING_PX * 2) / (fontSize * lineHeight)),
      ),
    }
  }

  context.font = `${fontSize}px ${fontFamily}`

  const cellWidth = Math.max(context.measureText("W").width + letterSpacing, 1)
  const cellHeight = Math.max(fontSize * lineHeight, 1)
  const usableWidth = Math.max(viewportWidth - VIEWPORT_PADDING_PX * 2, cellWidth)
  const usableHeight = Math.max(viewportHeight - VIEWPORT_PADDING_PX * 2, cellHeight)

  return {
    cols: Math.max(minimumCols, Math.floor(usableWidth / cellWidth)),
    rows: Math.max(minimumRows, Math.floor(usableHeight / cellHeight)),
  }
}

function translateKeyboardEvent(event: TargetedKeyboardEvent<HTMLDivElement>): string | null {
  if (event.isComposing || event.key === "Process" || event.metaKey) {
    return null
  }

  if (event.ctrlKey && event.altKey) {
    return null
  }

  if (event.ctrlKey && event.key.length === 1) {
    const uppercase = event.key.toUpperCase()
    const code = uppercase.charCodeAt(0)

    if (code >= 65 && code <= 90) {
      return String.fromCharCode(code - 64)
    }
  }

  switch (event.key) {
    case "Backspace":
      return "\u007f"
    case "Delete":
      return "\u001b[3~"
    case "ArrowDown":
      return "\u001b[B"
    case "ArrowLeft":
      return "\u001b[D"
    case "ArrowRight":
      return "\u001b[C"
    case "ArrowUp":
      return "\u001b[A"
    case "End":
      return "\u001b[F"
    case "Enter":
      return "\r"
    case "Escape":
      return "\u001b"
    case "Home":
      return "\u001b[H"
    case "Insert":
      return "\u001b[2~"
    case "PageDown":
      return "\u001b[6~"
    case "PageUp":
      return "\u001b[5~"
    case "Tab":
      return event.shiftKey ? "\u001b[Z" : "\t"
    default:
      break
  }

  if (event.key.length !== 1) {
    return null
  }

  if (event.altKey) {
    return `\u001b${event.key}`
  }

  return event.key
}

function writeToTerminal(terminal: Terminal, data: string | Uint8Array): Promise<void> {
  return new Promise((resolve) => {
    terminal.write(data, () => {
      resolve()
    })
  })
}
