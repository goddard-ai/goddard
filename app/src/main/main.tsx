import "@goddard-ai/styled-system/styles.css"
import { QueryClient, QueryClientProvider } from "@tanstack/preact-query"
import { render } from "preact"
import { AppShell } from "~/app-shell"
import { AppStateProvider } from "~/app-state-context"
import { desktopHost, initializeDesktopHost } from "~/desktop-host"

const queryClient = new QueryClient()

initializeDesktopHost()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <QueryClientProvider client={queryClient}>
    <AppStateProvider>
      <AppShell />
    </AppStateProvider>
  </QueryClientProvider>,
  document.getElementById("root")!,
)
