import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useListener, useSigma } from "preact-sigma"
import { useEffect, useRef, useState } from "preact/hooks"

import { TerminalViewportModel, type TerminalViewportChunk } from "./terminal-viewport-model.ts"
import { TerminalViewport } from "./terminal-viewport.tsx"

const terminalDebugPrompt = "debug@goddard:~/workspace$ "

const debugPageClass = css({
  display: "grid",
  gridTemplateRows: "auto minmax(0, 1fr)",
  gap: "18px",
  height: "100%",
  padding: "24px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 30%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

const debugHeaderClass = css({
  display: "grid",
  gap: "14px",
  padding: "24px 26px",
  borderRadius: "26px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
  boxShadow: "0 22px 56px rgba(118, 133, 150, 0.12)",
})

const debugEyebrowClass = css({
  display: "inline-flex",
  alignItems: "center",
  width: "fit-content",
  padding: "7px 11px",
  borderRadius: "999px",
  backgroundColor: "surface",
  color: "accentStrong",
  fontSize: "0.72rem",
  fontWeight: "720",
  letterSpacing: "0.15em",
  textTransform: "uppercase",
})

const debugTitleClass = css({
  color: "text",
  fontSize: "1.6rem",
  fontWeight: "760",
  letterSpacing: "-0.03em",
  lineHeight: "1.08",
})

const debugBodyClass = css({
  maxWidth: "76ch",
  color: "muted",
  fontSize: "0.95rem",
  lineHeight: "1.72",
})

const debugMetaRowClass = css({
  display: "flex",
  flexWrap: "wrap",
  gap: "10px",
})

const debugMetaChipClass = css({
  display: "inline-flex",
  alignItems: "center",
  padding: "8px 12px",
  borderRadius: "999px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "surface",
  color: "text",
  fontFamily: '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace',
  fontSize: "0.76rem",
})

const debugCanvasClass = css({
  minHeight: "0",
  overflow: "hidden",
  borderRadius: "28px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  boxShadow: "0 28px 64px rgba(118, 133, 150, 0.14)",
})

/** Props for the standalone terminal debug view. */
export type TerminalDebugViewProps = Record<string, never>

/** Renders the standalone terminal-debug surface used by the native development menu. */
export function TerminalDebugView(_props: TerminalDebugViewProps) {
  const terminal = useSigma(
    () => new TerminalViewportModel(),
    [
      {
        minimumCols: 56,
        minimumRows: 14,
      },
    ],
  )
  const terminalChunks = useRef<TerminalViewportChunk[]>([])
  const nextChunkId = useRef(0)
  const inputBuffer = useRef("")
  const terminalSizeRef = useRef({ cols: 56, rows: 14 })
  const [terminalSize, setTerminalSize] = useState(terminalSizeRef.current)
  const [lastCommand, setLastCommand] = useState<string | null>(null)

  function appendChunk(data: string) {
    nextChunkId.current += 1
    terminalChunks.current.push({
      id: `debug-terminal:${nextChunkId.current}`,
      data,
    })
    terminal.syncChunks(terminalChunks.current)
  }

  function resetTerminal() {
    nextChunkId.current = 0
    terminalChunks.current = []
    terminal.syncChunks(terminalChunks.current)
  }

  function appendPrompt() {
    appendChunk(terminalDebugPrompt)
  }

  function runCommand(rawCommand: string) {
    const command = rawCommand.trim()
    setLastCommand(command.length > 0 ? command : null)

    if (command === "clear") {
      resetTerminal()
      appendPrompt()
      return
    }

    if (command.length === 0) {
      appendPrompt()
      return
    }

    switch (command) {
      case "help":
        appendChunk(
          [
            "Available commands:",
            "  help   show the fixture-shell commands",
            "  ls     print a fake workspace listing",
            "  pwd    show the debug working directory",
            "  status show the current viewport dimensions",
            "  theme  render ANSI color samples",
            "  clear  clear the viewport",
            "  exit   explain how to leave the debug surface",
          ].join("\r\n") + "\r\n",
        )
        break
      case "ls":
        appendChunk("app  core  spec  workforce  package.json  bun.lock\r\n")
        break
      case "pwd":
        appendChunk("/Users/alec/.codex/worktrees/6e44/goddard-ai\r\n")
        break
      case "status":
        appendChunk(
          [
            `viewport: ${terminalSizeRef.current.cols} cols x ${terminalSizeRef.current.rows} rows`,
            "mode: fixture shell",
            "transport: local debug chunks",
          ].join("\r\n") + "\r\n",
        )
        break
      case "theme":
        appendChunk(
          [
            "\u001b[1mANSI color sample\u001b[0m",
            "\u001b[31mred\u001b[0m  \u001b[32mgreen\u001b[0m  \u001b[33myellow\u001b[0m  \u001b[34mblue\u001b[0m",
            "\u001b[35mmagenta\u001b[0m  \u001b[36mcyan\u001b[0m  \u001b[37mwhite\u001b[0m  \u001b[90mbright-black\u001b[0m",
          ].join("\r\n") + "\r\n",
        )
        break
      case "exit":
        appendChunk("Close the detail tab to leave this debug-only terminal surface.\r\n")
        break
      default:
        appendChunk(`command not found: ${command}\r\n`)
        break
    }

    appendPrompt()
  }

  function consumeInput(data: string) {
    let index = 0

    while (index < data.length) {
      if (data.charCodeAt(index) === 27 && data[index + 1] === "[") {
        index += 2

        while (index < data.length) {
          const code = data.charCodeAt(index)

          if (data[index] === "~" || (code >= 65 && code <= 90) || (code >= 97 && code <= 122)) {
            index += 1
            break
          }

          index += 1
        }

        continue
      }

      const character = data[index]

      if (character === "\r" || character === "\n") {
        if (character === "\n" && data[index - 1] === "\r") {
          index += 1
          continue
        }

        appendChunk("\r\n")
        const command = inputBuffer.current
        inputBuffer.current = ""
        runCommand(command)
        index += 1
        continue
      }

      if (character === "\u0003") {
        inputBuffer.current = ""
        appendChunk("^C\r\n")
        appendPrompt()
        index += 1
        continue
      }

      if (character === "\u007f") {
        if (inputBuffer.current.length > 0) {
          inputBuffer.current = inputBuffer.current.slice(0, -1)
          appendChunk("\b \b")
        }

        index += 1
        continue
      }

      if (character === "\t") {
        inputBuffer.current += "  "
        appendChunk("  ")
        index += 1
        continue
      }

      if (character.charCodeAt(0) < 32) {
        index += 1
        continue
      }

      inputBuffer.current += character
      appendChunk(character)
      index += 1
    }
  }

  useEffect(() => {
    const nextSize = { cols: terminal.cols, rows: terminal.rows }
    terminalSizeRef.current = nextSize
    setTerminalSize(nextSize)
    inputBuffer.current = ""
    setLastCommand(null)
    resetTerminal()
    appendChunk(
      [
        "Goddard terminal debug",
        "This fixture shell drives the real terminal viewport without host-side PTY state.",
        "Type help, ls, pwd, status, theme, clear, or exit to exercise rendering and input.",
        "",
      ].join("\r\n"),
    )
    appendPrompt()
  }, [terminal])

  useListener(terminal, "input", ({ data }) => {
    consumeInput(data)
  })

  useListener(terminal, "resize", (nextSize) => {
    terminalSizeRef.current = nextSize
    setTerminalSize(nextSize)
  })

  return (
    <div class={debugPageClass}>
      <section class={debugHeaderClass}>
        <div class={debugEyebrowClass}>Debug Surface</div>
        <h1 class={debugTitleClass}>Terminal</h1>
        <p class={debugBodyClass}>
          This view exercises the real terminal viewport and headless xterm model with a local
          fixture shell. It is intentionally disconnected from Bun-side PTY ownership so input echo,
          ANSI styling, and resize behavior can be tuned in isolation first.
        </p>
        <div class={debugMetaRowClass}>
          <div class={debugMetaChipClass}>{`${terminalSize.cols} x ${terminalSize.rows}`}</div>
          <div class={debugMetaChipClass}>{`last command: ${lastCommand ?? "none"}`}</div>
          <div class={debugMetaChipClass}>mode: fixture shell</div>
        </div>
      </section>
      <section class={debugCanvasClass}>
        <TerminalViewport ariaLabel="Terminal debug viewport" terminal={terminal} />
      </section>
    </div>
  )
}

export { TerminalDebugView as default }
