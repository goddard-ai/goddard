import "preact/debug"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsrx"
import { AppStateProvider } from "~/app-state-context.tsrx"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"

initializeDesktopHost()
startQueryWindowReactivationRefetch()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

const appRoot = <tsrx>
  <AppStateProvider>
    <AppShell />
  </AppStateProvider>
</tsrx>

render(
  appRoot,
  document.getElementById("root")!,
)
