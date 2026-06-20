type RendererLogCaptureWindow = Window & {
  __goddardDidInstallLogCapture?: boolean
}

export function isRendererLogCaptureInstalled() {
  if (typeof window === "undefined") {
    return false
  }

  return Boolean((window as RendererLogCaptureWindow).__goddardDidInstallLogCapture)
}

export function markRendererLogCaptureInstalled() {
  if (typeof window === "undefined") {
    return
  }

  const rendererWindow = window as RendererLogCaptureWindow
  rendererWindow.__goddardDidInstallLogCapture = true
}
