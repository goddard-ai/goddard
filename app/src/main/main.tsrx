import "preact/debug"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsrx"
import { AppStateProvider } from "~/app-state-context.tsrx"
import { getInitialAppearanceState } from "~/app-state-persistence.ts"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"

const initialAppearanceState = getInitialAppearanceState()

initializeDesktopHost()
startQueryWindowReactivationRefetch()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <>
    <AppStateProvider initialAppearanceState={initialAppearanceState}>
      <AppShell />
    </AppStateProvider>
  </>,
  document.getElementById("root")!,
)
