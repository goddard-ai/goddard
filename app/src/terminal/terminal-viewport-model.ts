import { Terminal, type ITheme } from "@xterm/headless"
import { SigmaTarget } from "preact-sigma"

const VIEWPORT_PADDING_PX = 18
const DEFAULT_MINIMUM_COLS = 40
const DEFAULT_MINIMUM_ROWS = 8
const DEFAULT_FONT_FAMILY = '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace'
const DEFAULT_FONT_SIZE = 13
const DEFAULT_LINE_HEIGHT = 1.45
const DEFAULT_LETTER_SPACING = 0

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

export type TerminalViewportChunk = {
  id: string
  data: string | Uint8Array
}

export type TerminalViewportFont = {
  family?: string
  size?: number
  lineHeight?: number
  letterSpacing?: number
}

export type TerminalViewportSetup = {
  theme?: ITheme
  font?: TerminalViewportFont
  minimumCols?: number
  minimumRows?: number
}

export type TerminalViewportSegment = {
  key: string
  style: preact.CSSProperties
  text: string
}

export type TerminalViewportRow = {
  key: string
  segments: TerminalViewportSegment[]
}

type TerminalViewportState = {
  cols: number
  rows: number
  viewRows: TerminalViewportRow[]
  fontFamily: string
  fontSize: number
  lineHeight: number
  letterSpacing: number
  theme: Readonly<ITheme>
  minimumCols: number
  minimumRows: number
}

type TerminalViewportEvents = {
  focus: void
  input: { data: string }
  paste: { data: string }
  resize: { cols: number; rows: number }
}

type TerminalCell = ReturnType<Terminal["buffer"]["active"]["getNullCell"]>

/** Long-lived terminal model that can outlive any single viewport mount. */
export class TerminalViewportModel extends SigmaTarget<
  TerminalViewportEvents,
  TerminalViewportState
> {
  /** Headless xterm instance that owns parser and buffer state behind the rendered row snapshot. */
  #terminal: Terminal | null = null
  /** Mounted viewport element used for measurement; DOM nodes are not serializable Sigma state. */
  #viewportElement: HTMLDivElement | null = null
  /** Disposable observer attached to the current viewport element for fit recalculation. */
  #resizeObserver: ResizeObserver | null = null
  /** Cursor into append-only output chunks already written into the terminal buffer. */
  #processedChunkCount = 0
  /** Monotonic token used to ignore stale async writes after resets or newer sync passes. */
  #writeVersion = 0

  constructor(config: TerminalViewportSetup = {}) {
    super({
      cols: DEFAULT_MINIMUM_COLS,
      rows: DEFAULT_MINIMUM_ROWS,
      viewRows: [],
      fontFamily: config.font?.family ?? DEFAULT_FONT_FAMILY,
      fontSize: config.font?.size ?? DEFAULT_FONT_SIZE,
      lineHeight: config.font?.lineHeight ?? DEFAULT_LINE_HEIGHT,
      letterSpacing: config.font?.letterSpacing ?? DEFAULT_LETTER_SPACING,
      theme: mergeTerminalTheme(config.theme),
      minimumCols: config.minimumCols ?? DEFAULT_MINIMUM_COLS,
      minimumRows: config.minimumRows ?? DEFAULT_MINIMUM_ROWS,
    })
  }

  /** Rebuilds the renderable rows from the current terminal buffer. */
  refreshSnapshot() {
    if (!this.#terminal) {
      this.cols = 0
      this.rows = 0
      this.viewRows = []
      return
    }

    const nextSnapshot = buildViewportSnapshot(this.#terminal, this.theme)
    this.cols = nextSnapshot.cols
    this.rows = nextSnapshot.rows
    this.viewRows = nextSnapshot.viewRows
  }

  /** Attaches or detaches one DOM viewport without affecting terminal lifetime. */
  attachViewport(viewportElement: HTMLDivElement | null) {
    if (this.#viewportElement === viewportElement) {
      return
    }

    this.#resizeObserver?.disconnect()
    this.#resizeObserver = null
    this.#viewportElement = viewportElement

    if (!viewportElement) {
      return
    }

    this.fitViewport()

    const observer = new ResizeObserver(() => {
      this.fitViewport()
    })

    observer.observe(viewportElement)
    this.#resizeObserver = observer
  }

  /** Recomputes terminal dimensions from the current viewport box. */
  fitViewport() {
    if (!this.#terminal || !this.#viewportElement) {
      return
    }

    const nextSize = measureTerminalSize(
      this.#viewportElement.clientWidth,
      this.#viewportElement.clientHeight,
      this.fontFamily,
      this.fontSize,
      this.lineHeight,
      this.letterSpacing,
      this.minimumCols,
      this.minimumRows,
    )

    if (nextSize.cols !== this.#terminal.cols || nextSize.rows !== this.#terminal.rows) {
      this.#terminal.resize(nextSize.cols, nextSize.rows)
    }
  }

  /** Streams append-only PTY output chunks into the terminal. */
  syncChunks(chunks: readonly TerminalViewportChunk[]) {
    if (!this.#terminal) {
      return
    }

    if (chunks.length < this.#processedChunkCount) {
      this.#processedChunkCount = 0
      this.#writeVersion += 1
      this.#terminal.reset()
      this.#terminal.clear()
      this.refreshSnapshot()
    }

    if (chunks.length === this.#processedChunkCount) {
      return
    }

    const nextVersion = this.#writeVersion + 1
    this.#writeVersion = nextVersion

    void this.#syncTerminalChunks(chunks, this.#processedChunkCount, nextVersion)
  }

  /** Emits one terminal input event for the owner to forward to its PTY. */
  forwardInput(data: string) {
    this.emit("input", { data })
  }

  /** Emits paste and input events for the owner to forward to its PTY. */
  forwardPaste(data: string) {
    this.emit("paste", { data })
    this.emit("input", { data })
  }

  /** Emits one focus event for the owner to observe. */
  notifyFocus() {
    this.emit("focus")
  }

  /** Scrolls the visible viewport using one wheel delta. */
  scrollViewport(deltaY: number, deltaMode: number) {
    if (!this.#terminal) {
      return
    }

    const lines =
      deltaMode === WheelEvent.DOM_DELTA_LINE
        ? Math.trunc(deltaY)
        : Math.trunc(deltaY / Math.max(this.fontSize * this.lineHeight, 1))

    if (lines === 0) {
      return
    }

    this.#terminal.scrollLines(lines)
    this.refreshSnapshot()
  }

  /** Finalizes one successful async chunk sync on the public model state. */
  #finishChunkSync(chunksLength: number) {
    this.#processedChunkCount = chunksLength
    this.refreshSnapshot()
  }

  onSetup() {
    this.act(function () {
      if (this.#terminal) {
        return
      }

      this.#terminal = new Terminal({
        cols: Math.max(this.minimumCols, 1),
        rows: Math.max(this.minimumRows, 1),
        convertEol: true,
        cursorBlink: true,
        lineHeight: this.lineHeight,
        letterSpacing: this.letterSpacing,
        scrollback: 5000,
        theme: cloneTheme(this.theme),
      })

      this.refreshSnapshot()
    })

    if (!this.#terminal) {
      return []
    }

    return [
      this.#terminal.onScroll(() => {
        this.refreshSnapshot()
      }),
      this.#terminal.onWriteParsed(() => {
        this.refreshSnapshot()
      }),
      this.#terminal.onResize((nextSize: { cols: number; rows: number }) => {
        this.act(function () {
          this.refreshSnapshot()
          this.commit()
          this.emit("resize", { cols: nextSize.cols, rows: nextSize.rows })
        })
      }),
      this.#terminal.onTitleChange(() => {
        this.refreshSnapshot()
      }),
      () => {
        this.act(function () {
          this.#resizeObserver?.disconnect()
          this.#resizeObserver = null

          this.#terminal?.dispose()
          this.#terminal = null
          this.#viewportElement = null
          this.#processedChunkCount = 0
          this.#writeVersion += 1
          this.refreshSnapshot()
        })
      },
    ]
  }

  async #syncTerminalChunks(
    chunks: readonly TerminalViewportChunk[],
    startIndex: number,
    writeVersion: number,
  ) {
    if (!this.#terminal) {
      return
    }

    const terminal = this.#terminal

    for (const chunk of chunks.slice(startIndex)) {
      await writeToTerminal(terminal, chunk.data)

      if (this.#writeVersion !== writeVersion) {
        return
      }
    }

    this.#finishChunkSync(chunks.length)
  }
}

export interface TerminalViewportModel extends TerminalViewportState {}

export function translateKeyboardEvent(
  event: preact.TargetedKeyboardEvent<HTMLDivElement>,
): string | null {
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

function buildViewportSnapshot(
  terminal: Terminal,
  theme: ITheme,
): Pick<TerminalViewportState, "cols" | "rows" | "viewRows"> {
  const buffer = terminal.buffer.active
  const cursorLine = buffer.baseY + buffer.cursorY
  const viewRows: TerminalViewportRow[] = []

  for (let index = 0; index < terminal.rows; index += 1) {
    const absoluteLine = buffer.viewportY + index
    const line = buffer.getLine(absoluteLine)
    const segments: TerminalViewportSegment[] = []
    const nullCell = buffer.getNullCell()
    let currentText = ""
    let currentStyle: preact.CSSProperties | null = null
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

function cloneTheme(theme: Readonly<ITheme>): ITheme {
  return {
    ...theme,
    extendedAnsi: theme.extendedAnsi ? [...theme.extendedAnsi] : undefined,
  }
}

function resolveCellStyle(
  cell: TerminalCell | undefined,
  theme: ITheme,
  isCursor: boolean,
): preact.CSSProperties {
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

function serializeStyle(style: preact.CSSProperties): string {
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

function writeToTerminal(terminal: Terminal, data: string | Uint8Array): Promise<void> {
  return new Promise((resolve) => {
    terminal.write(data, () => {
      resolve()
    })
  })
}
