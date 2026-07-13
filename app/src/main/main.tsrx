import "preact/debug"
import { setOverlayPortalRoots } from "@goddard-ai/ui-primitives"
import { render } from "preact"

import { AppShell } from "~/app-shell.tsrx"
import { AppStateProvider } from "~/app-state-context.tsrx"
import { LaunchableStates } from "~/dev/view.tsrx"
import { desktopHost, initializeDesktopHost, mainWindowReady } from "~/desktop-host.ts"
import { activateLocale, defaultLocale } from "~/language/i18n.ts"
import { startQueryWindowReactivationRefetch } from "~/lib/query.ts"
import { writeRendererError } from "~/lib/renderer-log-capture.ts"

async function main() {
  initializeDesktopHost()
  await activateLocale(defaultLocale)

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

  requestAnimationFrame(() => {
    void mainWindowReady().catch((error) => {
      writeRendererError("app.renderer.ready_failed", error)
    })
  })
}

void main().catch((error) => {
  writeRendererError("app.renderer.startup_failed", error)
})
