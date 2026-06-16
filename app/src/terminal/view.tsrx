import { useListener, useSigma } from "preact-sigma"
import { useEffect } from "preact/hooks"

import { TerminalViewport } from "./terminal-viewport.tsrx"
import { getTerminalTab, type TerminalTabPayload } from "./tab.ts"
import styles from "./view.style.ts"

export function TerminalView(props: TerminalTabPayload) @{
  const terminal = useSigma(() => getTerminalTab(props), [props.terminalId])
  const viewport = useSigma(() => terminal.terminal, [terminal])
  const canWrite = terminal.status === "running"
  const canRestart = terminal.status !== "connecting" && terminal.status !== "restarting"
  const canClose = terminal.status !== "closed" && terminal.status !== "connecting"

  useEffect(() => {
    void terminal.start()
  }, [terminal])

  useListener(viewport, "input", ({ data }) => {
    void terminal.write(data).catch((error) => {
      console.error("Failed to write terminal input.", error)
    })
  })

  useListener(viewport, "resize", (dimensions) => {
    void terminal.resize(dimensions).catch((error) => {
      console.error("Failed to resize terminal.", error)
    })
  })

  <div class={styles.page}>
    <section class={styles.toolbar} aria-label="Terminal controls">
      <div class={styles.titleGroup}>
        <h1 class={styles.title}>{terminal.title}</h1>
        <div class={styles.meta}>
          {terminal.cwd ?? "daemon default shell directory"}
        </div>
      </div>
      <div class={styles.controls}>
        <span class={styles.chip}>{terminal.status}</span>
        <span class={styles.chip}>{`${terminal.cols} x ${terminal.rows}`}</span>
        <button
          class={styles.button}
          disabled={!canRestart}
          type="button"
          onClick={() => {
            void terminal.restart().catch((error) => {
              console.error("Failed to restart terminal.", error)
            })
          }}
        >{"Restart"}</button>
        <button
          class={styles.button}
          disabled={!canClose}
          type="button"
          onClick={() => {
            void terminal.close().catch((error) => {
              console.error("Failed to close terminal.", error)
            })
          }}
        >{"Close"}</button>
      </div>
    </section>

    @if (terminal.statusMessage) {
      <div class={styles.error}>{terminal.statusMessage}</div>
    }

    <TerminalViewport
      ariaLabel={canWrite ? "Daemon terminal viewport" : "Daemon terminal viewport inactive"}
      class={styles.viewport}
      terminal={viewport}
    />
  </div>
}

export default TerminalView
