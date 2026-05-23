/** Platform identifier used by desktop worktree opener discovery. */
export type DesktopPlatform = "macos" | "windows" | "linux"

/** Stable product-level identifier for one supported worktree opener. */
export type WorktreeOpenerId =
  | "cursor"
  | "vscode"
  | "vscode-insiders"
  | "windsurf"
  | "xcode"
  | "android-studio"
  | "intellij-idea"
  | "webstorm"
  | "pycharm"
  | "goland"
  | "phpstorm"
  | "sublime-text"
  | "zed"
  | "ghostty"
  | "terminal"
  | "finder"
  | "explorer"
  | "linux-files"

/** High-level category used by the renderer for labels and fallback icons. */
export type WorktreeOpenerKind = "ide" | "file-manager" | "terminal"

/** Renderer-facing worktree opener shape with native launch details intentionally omitted. */
export type WorktreeOpener = {
  id: WorktreeOpenerId
  displayName: string
  kind: WorktreeOpenerKind
  platform: DesktopPlatform
  available: boolean
  iconUrl?: string
  secondaryText?: string
}

/** Response returned when the renderer asks for available worktree openers. */
export type WorktreeOpenersResponse = {
  platform: DesktopPlatform
  openers: WorktreeOpener[]
  primaryOpenerId: WorktreeOpenerId | null
  persistedOpenerId: WorktreeOpenerId | null
}

/** Typed launch failure codes surfaced through the desktop RPC boundary. */
export type WorktreeOpenErrorCode =
  | "WORKTREE_NOT_FOUND"
  | "OPENER_NOT_FOUND"
  | "OPENER_UNAVAILABLE"
  | "LAUNCH_FAILED"
  | "PERMISSION_DENIED"
  | "UNKNOWN"

/** Launch result returned by the host after a user-triggered open action. */
export type OpenWorktreeResponse =
  | {
      ok: true
      openerId: WorktreeOpenerId
    }
  | {
      ok: false
      openerId: WorktreeOpenerId
      errorCode: WorktreeOpenErrorCode
      message: string
    }
