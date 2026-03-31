import { Terminal, type ITheme } from "@xterm/headless"
import type { CSSProperties, TargetedKeyboardEvent } from "preact"
import { SigmaType } from "preact-sigma"

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

export type TerminalViewportSegment = {
  key: string
  style: CSSProperties
  text: string
}

export type TerminalViewportRow = {
  key: string
  segments: TerminalViewportSegment[]
}

type TerminalViewportShape = {
  cols: number
  rows: number
  viewRows: TerminalViewportRow[]
  fontFamily: string
  fontSize: number
  lineHeight: number
  letterSpacing: number
  theme: ITheme
}

type TerminalViewportCallbacks = {
  onReady?: () => void
  onResize?: (cols: number, rows: number) => void
  onInput?: (data: string) => void
  onPaste?: (data: string) => void
  onFocus?: () => void
}

type TerminalViewportRuntime = {
  callbacks: TerminalViewportCallbacks
  minimumCols: number
  minimumRows: number
  processedChunkCount: number
  resizeObserver: ResizeObserver | null
  sessionId: string | null
  terminal: Terminal | null
  terminalDisposables: { dispose(): void }[]
  viewportElement: HTMLDivElement | null
  writeVersion: number
}

type TerminalViewportConfig = {
  sessionId: string
  theme?: ITheme
  font?: TerminalViewportFont
  minimumCols?: number
  minimumRows?: number
} & TerminalViewportCallbacks

type TerminalCell = ReturnType<Terminal["buffer"]["active"]["getNullCell"]>

const runtimeByViewport = new WeakMap<object, TerminalViewportRuntime>()

/** Sigma model that owns headless terminal lifecycle and renderable viewport state. */
export const TerminalViewportModel = new SigmaType<TerminalViewportShape>("TerminalViewportModel")
  .defaultState({
    cols: DEFAULT_MINIMUM_COLS,
    rows: DEFAULT_MINIMUM_ROWS,
    viewRows: [],
    fontFamily: DEFAULT_FONT_FAMILY,
    fontSize: DEFAULT_FONT_SIZE,
    lineHeight: DEFAULT_LINE_HEIGHT,
    letterSpacing: DEFAULT_LETTER_SPACING,
    theme: defaultTheme,
  })
  .setup(function () {
    ensureRuntime(this)

    return [() => disposeRuntime(this)]
  })
  .actions({
    /** Applies non-render state, callbacks, and viewport display options. */
    configureViewport(config: TerminalViewportConfig) {
      const runtime = ensureRuntime(this)
      const nextTheme = mergeTerminalTheme(config.theme)
      const nextFontFamily = config.font?.family ?? DEFAULT_FONT_FAMILY
      const nextFontSize = config.font?.size ?? DEFAULT_FONT_SIZE
      const nextLineHeight = config.font?.lineHeight ?? DEFAULT_LINE_HEIGHT
      const nextLetterSpacing = config.font?.letterSpacing ?? DEFAULT_LETTER_SPACING
      const sessionChanged = runtime.sessionId !== config.sessionId

      runtime.callbacks = {
        onReady: config.onReady,
        onResize: config.onResize,
        onInput: config.onInput,
        onPaste: config.onPaste,
        onFocus: config.onFocus,
      }
      runtime.minimumCols = config.minimumCols ?? DEFAULT_MINIMUM_COLS
      runtime.minimumRows = config.minimumRows ?? DEFAULT_MINIMUM_ROWS
      runtime.sessionId = config.sessionId

      this.fontFamily = nextFontFamily
      this.fontSize = nextFontSize
      this.lineHeight = nextLineHeight
      this.letterSpacing = nextLetterSpacing
      this.theme = nextTheme

      this.ensureTerminal()

      if (runtime.terminal) {
        runtime.terminal.options = {
          ...runtime.terminal.options,
          lineHeight: this.lineHeight,
          letterSpacing: this.letterSpacing,
          theme: this.theme,
        }
      }

      if (sessionChanged) {
        runtime.processedChunkCount = 0
        runtime.writeVersion += 1
        runtime.terminal?.reset()
        runtime.terminal?.clear()
      }

      this.fitViewport()
      this.refreshSnapshot()
    },

    /** Binds one viewport element and resize observer to this terminal instance. */
    attachViewport(viewportElement: HTMLDivElement | null) {
      const runtime = ensureRuntime(this)

      if (runtime.viewportElement === viewportElement) {
        return
      }

      runtime.resizeObserver?.disconnect()
      runtime.resizeObserver = null
      runtime.viewportElement = viewportElement

      if (!viewportElement) {
        return
      }

      this.ensureTerminal()
      this.fitViewport()

      const observer = new ResizeObserver(() => {
        this.fitViewport()
      })

      observer.observe(viewportElement)
      runtime.resizeObserver = observer
    },

    /** Streams append-only PTY output chunks into the headless terminal. */
    syncChunks(chunks: readonly TerminalViewportChunk[]) {
      const runtime = ensureRuntime(this)

      this.ensureTerminal()

      if (!runtime.terminal) {
        return
      }

      if (chunks.length < runtime.processedChunkCount) {
        runtime.processedChunkCount = 0
        runtime.writeVersion += 1
        runtime.terminal.reset()
        runtime.terminal.clear()
        this.refreshSnapshot()
      }

      if (chunks.length === runtime.processedChunkCount) {
        return
      }

      const nextVersion = runtime.writeVersion + 1
      runtime.writeVersion = nextVersion

      void syncTerminalChunks(this, chunks, runtime.processedChunkCount, nextVersion)
    },

    /** Creates the headless terminal lazily and subscribes its lifecycle events once. */
    ensureTerminal() {
      const runtime = ensureRuntime(this)

      if (runtime.terminal) {
        return
      }

      const terminal = new Terminal({
        cols: Math.max(runtime.minimumCols, 1),
        rows: Math.max(runtime.minimumRows, 1),
        convertEol: true,
        cursorBlink: true,
        lineHeight: this.lineHeight,
        letterSpacing: this.letterSpacing,
        scrollback: 5000,
        theme: this.theme,
      })

      runtime.terminal = terminal

      runtime.terminalDisposables = [
        terminal.onScroll(() => {
          this.refreshSnapshot()
        }),
        terminal.onWriteParsed(() => {
          this.refreshSnapshot()
        }),
        terminal.onResize((nextSize) => {
          this.refreshSnapshot()
          ensureRuntime(this).callbacks.onResize?.(nextSize.cols, nextSize.rows)
        }),
        terminal.onTitleChange(() => {
          this.refreshSnapshot()
        }),
      ]

      this.refreshSnapshot()
      runtime.callbacks.onReady?.()
    },

    /** Rebuilds the renderable viewport snapshot from the headless xterm buffer. */
    refreshSnapshot() {
      const terminal = ensureRuntime(this).terminal

      if (!terminal) {
        this.cols = 0
        this.rows = 0
        this.viewRows = []
        return
      }

      const nextSnapshot = buildViewportSnapshot(terminal, this.theme)
      this.cols = nextSnapshot.cols
      this.rows = nextSnapshot.rows
      this.viewRows = nextSnapshot.viewRows
    },

    /** Recomputes columns and rows from the current viewport size and font metrics. */
    fitViewport() {
      const runtime = ensureRuntime(this)

      if (!runtime.terminal || !runtime.viewportElement) {
        return
      }

      const nextSize = measureTerminalSize(
        runtime.viewportElement.clientWidth,
        runtime.viewportElement.clientHeight,
        this.fontFamily,
        this.fontSize,
        this.lineHeight,
        this.letterSpacing,
        runtime.minimumCols,
        runtime.minimumRows,
      )

      if (nextSize.cols !== runtime.terminal.cols || nextSize.rows !== runtime.terminal.rows) {
        runtime.terminal.resize(nextSize.cols, nextSize.rows)
      }
    },

    /** Forwards one translated keyboard input string to the owning transport. */
    forwardInput(data: string) {
      ensureRuntime(this).callbacks.onInput?.(data)
    },

    /** Forwards one pasted string to the owning transport and paste hook. */
    forwardPaste(data: string) {
      const callbacks = ensureRuntime(this).callbacks

      callbacks.onPaste?.(data)
      callbacks.onInput?.(data)
    },

    /** Forwards viewport focus to the owning transport host. */
    notifyFocus() {
      ensureRuntime(this).callbacks.onFocus?.()
    },

    /** Scrolls the visible terminal viewport based on one wheel delta. */
    scrollViewport(deltaY: number, deltaMode: number) {
      const terminal = ensureRuntime(this).terminal

      if (!terminal) {
        return
      }

      const lines =
        deltaMode === WheelEvent.DOM_DELTA_LINE
          ? Math.trunc(deltaY)
          : Math.trunc(deltaY / Math.max(this.fontSize * this.lineHeight, 1))

      if (lines === 0) {
        return
      }

      terminal.scrollLines(lines)
      this.refreshSnapshot()
    },
  })

/** Runtime instance type for the terminal viewport sigma model. */
export interface TerminalViewportModel extends InstanceType<typeof TerminalViewportModel> {}

export function translateKeyboardEvent(
  event: TargetedKeyboardEvent<HTMLDivElement>,
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

function ensureRuntime(owner: object): TerminalViewportRuntime {
  const existingRuntime = runtimeByViewport.get(owner)

  if (existingRuntime) {
    return existingRuntime
  }

  const nextRuntime: TerminalViewportRuntime = {
    callbacks: {},
    minimumCols: DEFAULT_MINIMUM_COLS,
    minimumRows: DEFAULT_MINIMUM_ROWS,
    processedChunkCount: 0,
    resizeObserver: null,
    sessionId: null,
    terminal: null,
    terminalDisposables: [],
    viewportElement: null,
    writeVersion: 0,
  }

  runtimeByViewport.set(owner, nextRuntime)
  return nextRuntime
}

function disposeRuntime(owner: object): void {
  const runtime = runtimeByViewport.get(owner)

  if (!runtime) {
    return
  }

  runtime.resizeObserver?.disconnect()

  for (const disposable of runtime.terminalDisposables) {
    disposable.dispose()
  }

  runtime.terminal?.dispose()
  runtimeByViewport.delete(owner)
}

async function syncTerminalChunks(
  model: { refreshSnapshot: () => void },
  chunks: readonly TerminalViewportChunk[],
  startIndex: number,
  writeVersion: number,
): Promise<void> {
  const runtime = ensureRuntime(model)
  const terminal = runtime.terminal

  if (!terminal) {
    return
  }

  for (const chunk of chunks.slice(startIndex)) {
    await writeToTerminal(terminal, chunk.data)

    if (ensureRuntime(model).writeVersion !== writeVersion) {
      return
    }
  }

  ensureRuntime(model).processedChunkCount = chunks.length
  model.refreshSnapshot()
}

function buildViewportSnapshot(
  terminal: Terminal,
  theme: ITheme,
): Pick<TerminalViewportShape, "cols" | "rows" | "viewRows"> {
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

function writeToTerminal(terminal: Terminal, data: string | Uint8Array): Promise<void> {
  return new Promise((resolve) => {
    terminal.write(data, () => {
      resolve()
    })
  })
}
