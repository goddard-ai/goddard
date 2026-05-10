type Rectangle = {
  x: number
  y: number
  width: number
  height: number
}

type Display = {
  id: number
  bounds: Rectangle
  workArea: Rectangle
  scaleFactor: number
  isPrimary: boolean
}

type OverlayWindow = {
  hide(): void
  setAlwaysOnTop(alwaysOnTop: boolean): void
  setFrame(x: number, y: number, width: number, height: number): void
  show(): void
}

/** Minimal native-global-shortcut API used by the overlay host coordinator. */
export type GlobalShortcutDriver = {
  isRegistered(accelerator: string): boolean
  register(accelerator: string, callback: () => void): boolean
  unregister(accelerator: string): boolean
}

/** Minimal display and pointer API needed to target the active display. */
export type ScreenDriver = {
  getAllDisplays(): Display[]
  getCursorScreenPoint(): { x: number; y: number }
  getPrimaryDisplay(): Display
}

/** Native window options the overlay host requires from the concrete window adapter. */
export type OverlayWindowOptions<TRpc> = {
  activate: boolean
  frame: Rectangle
  hidden: boolean
  passthrough: boolean
  renderer: "cef"
  rpc: TRpc
  styleMask: {
    Borderless: boolean
    Titled: boolean
    Closable: boolean
    Miniaturizable: boolean
    Resizable: boolean
    FullSizeContentView: boolean
  }
  title: string
  titleBarStyle: "hidden"
  transparent: boolean
  url: string
}

/** Creates one native overlay window from normalized overlay options. */
export type OverlayWindowFactory<TRpc> = (options: OverlayWindowOptions<TRpc>) => OverlayWindow

/** Runtime dependencies and callbacks supplied by the Bun app entrypoint. */
export type GlobalSessionLaunchOverlayHostOptions<TRpc> = {
  getOverlayUrl: () => string
  onShortcut: () => void
  rpc: TRpc
}

/** Result of attempting to claim the user-selected native global shortcut. */
export type GlobalSessionLaunchShortcutRegistration =
  | { registered: true }
  | { registered: false; reason: "unavailable" }

function containsPoint(rectangle: Rectangle, point: { x: number; y: number }) {
  return (
    point.x >= rectangle.x &&
    point.y >= rectangle.y &&
    point.x < rectangle.x + rectangle.width &&
    point.y < rectangle.y + rectangle.height
  )
}

/** Resolves the display that currently owns the pointer, falling back to the primary display. */
export function findActiveDisplay(screen: ScreenDriver) {
  const cursorPoint = screen.getCursorScreenPoint()
  const display = screen
    .getAllDisplays()
    .find((candidate) => containsPoint(candidate.bounds, cursorPoint))

  return display ?? screen.getPrimaryDisplay()
}

/** Builds the native window options required for the transparent launch overlay. */
export function createOverlayWindowOptions<TRpc>(
  display: Display,
  options: GlobalSessionLaunchOverlayHostOptions<TRpc>,
) {
  return {
    title: "Goddard Launch Session",
    titleBarStyle: "hidden",
    url: options.getOverlayUrl(),
    rpc: options.rpc,
    renderer: "cef",
    frame: display.bounds,
    transparent: true,
    passthrough: false,
    hidden: true,
    activate: false,
    styleMask: {
      Borderless: true,
      Titled: false,
      Closable: false,
      Miniaturizable: false,
      Resizable: false,
      FullSizeContentView: true,
    },
  } satisfies OverlayWindowOptions<TRpc>
}

/** Coordinates global shortcut ownership and overlay window visibility. */
export function createGlobalSessionLaunchOverlayHost<TRpc>(
  options: GlobalSessionLaunchOverlayHostOptions<TRpc>,
  drivers: {
    createWindow: OverlayWindowFactory<TRpc>
    globalShortcut: GlobalShortcutDriver
    screen: ScreenDriver
  },
) {
  let registeredAccelerator: string | null = null
  let overlayWindow: OverlayWindow | null = null
  let visible = false

  function unregisterGlobalShortcut() {
    if (registeredAccelerator === null) {
      return
    }

    drivers.globalShortcut.unregister(registeredAccelerator)
    registeredAccelerator = null
  }

  function registerGlobalShortcut(accelerator: string): GlobalSessionLaunchShortcutRegistration {
    if (
      registeredAccelerator === accelerator &&
      drivers.globalShortcut.isRegistered(accelerator)
    ) {
      return { registered: true }
    }

    unregisterGlobalShortcut()

    const didRegister = drivers.globalShortcut.register(accelerator, options.onShortcut)

    if (!didRegister) {
      return { registered: false, reason: "unavailable" }
    }

    registeredAccelerator = accelerator
    return { registered: true }
  }

  function getOverlayWindow() {
    if (overlayWindow) {
      return overlayWindow
    }

    overlayWindow = drivers.createWindow(
      createOverlayWindowOptions(findActiveDisplay(drivers.screen), options),
    )
    overlayWindow.setAlwaysOnTop(true)
    return overlayWindow
  }

  function showOverlay() {
    const display = findActiveDisplay(drivers.screen)
    const window = getOverlayWindow()

    window.setFrame(
      display.bounds.x,
      display.bounds.y,
      display.bounds.width,
      display.bounds.height,
    )
    window.setAlwaysOnTop(true)
    window.show()
    visible = true
  }

  function hideOverlay() {
    overlayWindow?.hide()
    visible = false
  }

  function toggleOverlay() {
    if (visible) {
      hideOverlay()
      return
    }

    showOverlay()
  }

  function dispose() {
    unregisterGlobalShortcut()
    hideOverlay()
  }

  return {
    dispose,
    hideOverlay,
    isOverlayVisible: () => visible,
    registerGlobalShortcut,
    showOverlay,
    toggleOverlay,
    unregisterGlobalShortcut,
  }
}
