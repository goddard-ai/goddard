import { useEffect, useState } from "preact/hooks"
import { getRuntimeInfo } from "./desktop-host"

/** Current state of the Electrobun runtime handshake shown in the setup UI. */
type RuntimeStatus =
  | { kind: "loading" }
  | { kind: "ready"; runtime: "electrobun" }
  | { kind: "error"; message: string }

/** Renders a small runtime proof that the webview can call the Bun host over RPC. */
function App() {
  const [runtimeStatus, setRuntimeStatus] = useState<RuntimeStatus>({ kind: "loading" })

  useEffect(() => {
    let cancelled = false

    void getRuntimeInfo()
      .then((runtimeInfo) => {
        if (!cancelled) {
          setRuntimeStatus({
            kind: "ready",
            runtime: runtimeInfo.runtime,
          })
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setRuntimeStatus({
            kind: "error",
            message: error instanceof Error ? error.message : String(error),
          })
        }
      })

    return () => {
      cancelled = true
    }
  }, [])

  const bridgeStatus =
    runtimeStatus.kind === "ready"
      ? "ready"
      : runtimeStatus.kind === "error"
        ? "error"
        : "connecting"

  return (
    <main className="container">
      <section className="status-panel">
        <p className="eyebrow">Electrobun Runtime</p>
        <h1>Desktop host connected through Bun RPC.</h1>
        <p className="lede">
          This app now boots through Electrobun, with the webview talking to the desktop host over a
          typed RPC bridge.
        </p>

        <div className="status-grid">
          <article className="status-card">
            <span className="status-label">Runtime</span>
            <strong className="status-value">
              {runtimeStatus.kind === "ready" ? runtimeStatus.runtime : "pending"}
            </strong>
          </article>

          <article className="status-card">
            <span className="status-label">Bridge</span>
            <strong className="status-value">{bridgeStatus}</strong>
          </article>
        </div>

        <p className="helper-text">
          Manual daemon smoke test: call{" "}
          <code>window.__goddardDesktop.sdk.daemon.health(&#123;&#125;)</code> from the devtools
          console.
        </p>

        {runtimeStatus.kind === "error" ? (
          <p className="error-text">{runtimeStatus.message}</p>
        ) : null}
      </section>
    </main>
  )
}

export default App
