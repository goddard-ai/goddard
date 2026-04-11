import "preact/debug"
import "@goddard-ai/styled-system/styles.css"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsx"
import { AppStateProvider } from "~/app-state-context.tsx"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"

initializeDesktopHost()
startQueryWindowReactivationRefetch()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <AppStateProvider>
    <AppShell />
  </AppStateProvider>,
  document.getElementById("root")!,
)
