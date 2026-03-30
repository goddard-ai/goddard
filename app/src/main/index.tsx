import "@goddard-ai/styled-system/styles.css"
import { QueryClient, QueryClientProvider } from "@tanstack/preact-query"
import { render } from "preact"
import App from "../App"
import { desktopHost, initializeDesktopHost } from "../desktop-host"

const queryClient = new QueryClient()

initializeDesktopHost()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <QueryClientProvider client={queryClient}>
    <App />
  </QueryClientProvider>,
  document.getElementById("root")!,
)
