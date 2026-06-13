import "preact/debug"
import { setOverlayPortalRoots } from "@goddard-ai/ui-primitives"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsrx"
import { AppStateProvider } from "~/app-state-context.tsrx"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"
import { LaunchableStates } from "~/launchable-states/view.tsrx"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"

initializeDesktopHost()
startQueryWindowReactivationRefetch()
setOverlayPortalRoots({
  dialog: () => document.getElementById("dialog-portal"),
  menu: () => document.getElementById("menu-portal"),
})

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <>
    <AppStateProvider>
      {import.meta.env.DEV ? <LaunchableStates /> : null}
      <AppShell />
    </AppStateProvider>
  </>,
  document.getElementById("root")!,
)
