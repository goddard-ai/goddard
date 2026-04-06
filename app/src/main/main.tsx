import "@goddard-ai/styled-system/styles.css"
import { QueryClient, QueryClientProvider } from "@tanstack/preact-query"
import { render } from "preact"
import { AppShell } from "~/app-shell.tsx"
import { AppStateProvider } from "~/app-state-context.tsx"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"

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
