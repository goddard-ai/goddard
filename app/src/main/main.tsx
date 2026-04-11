import "preact/debug"
import "@goddard-ai/styled-system/styles.css"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsx"
import { AppStateProvider } from "~/app-state-context.tsx"
import { getInitialAppearanceSnapshot } from "~/appearance/theme.ts"
import { desktopHost, initializeDesktopHost } from "~/desktop-host.ts"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"

const initialAppearanceSnapshot = getInitialAppearanceSnapshot()

initializeDesktopHost()
startQueryWindowReactivationRefetch()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(
  <AppStateProvider initialAppearanceSnapshot={initialAppearanceSnapshot}>
    <AppShell />
  </AppStateProvider>,
  document.getElementById("root")!,
)
