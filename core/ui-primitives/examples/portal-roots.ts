import { setOverlayPortalRoots } from "@goddard-ai/ui-primitives"

/**
 * Configure overlay host elements once from application setup code.
 * Reusable libraries should not call setOverlayPortalRoots.
 */
export function configureOverlayPortalRoots() {
  setOverlayPortalRoots({
    dialog: () => document.getElementById("dialog-root"),
    menu: () => document.getElementById("overlay-root"),
  })
}
