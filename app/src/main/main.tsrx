import "preact/debug"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsrx"
import { AppStateProvider } from "~/app-state-context.tsrx"
import { getInitialAppearanceSnapshot } from "~/app-state-persistence.ts"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"

const initialAppearanceSnapshot = getInitialAppearanceSnapshot()

initializeDesktopHost()
startQueryWindowReactivationRefetch()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <tsx>
    <AppStateProvider initialAppearanceSnapshot={initialAppearanceSnapshot}>
      <AppShell />
    </AppStateProvider>
  </tsx>,
  document.getElementById("root")!,
)
