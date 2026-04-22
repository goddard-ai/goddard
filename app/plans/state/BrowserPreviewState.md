# State Module: BrowserPreviewState

- **Responsibility:** Manage iframe-driven preview sessions, including URL entry, committed navigation history, loading state, and observable console logs from the custom protocol shim.
- **Data Shape:** One map keyed by preview id containing display URL text, committed URL, back and forward history stacks or index state, loading status, page metadata, navigation errors, console entries, console filters, and cached pane-layout preferences.
- **Mutations/Actions:** `createPreview`; `setDisplayUrl`; `navigate`; `commitNavigation`; `goBack`; `goForward`; `refresh`; `stopNavigation`; `appendConsoleEntry`; `clearConsole`; `closePreview`.
- **Scope & Hoisting:** Hoisted into a shared provider keyed by preview id so preview tabs can preserve history and console logs when hidden or restored from cache.
- **Side Effects:** Invokes preview-specific Electrobun RPC calls or Bun-side services such as `preview.navigate`, `preview.reload`, `preview.stop`, and any setup needed for the Bun-hosted preview protocol layer; listens for iframe `postMessage` console telemetry; depends on host-side protocol middleware to rewrite headers and inject the console shim. That dependency still needs alignment with the current Electrobun host boundary before implementation.
